//! Outgoing TCP and UDP for components, restricted to the destinations PHP
//! allows.
//!
//! wasmtime-wasi asks before each socket operation with only an IP address
//! and a port, since the guest resolves names itself. A TCP name rule is
//! therefore checked by resolving it on the host when the guest connects. UDP
//! name rules are resolved once, when the rules are set up: wasmtime-wasi
//! reports a send whose check is still pending as sent and the refusal only on
//! the next send (p2/udp.rs, poll_or_spawn), so the check for a send has to be
//! ready on its first poll.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use ext_php_rs::exception::PhpResult;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::sockets::SocketAddrUse;

use crate::component::http::{normalize, split_port};
use crate::error::{type_error, value_error};
use crate::value::debug_type;

/// One `tcpHosts` or `udpHosts` entry: a name, an address or a network, and a
/// port or any.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    target: Target,
    /// `None` for `*`, any port.
    port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Name(String),
    Network(IpAddr, u8),
}

const SHAPE: &str =
    "must be a host, address or network with a port, such as db.internal:5432 or 10.0.0.0/8:*";

impl Rule {
    /// `list` names the constructor argument in errors.
    pub fn parse(entry: &str, list: &str) -> Result<Self, String> {
        let invalid = |reason: &str| format!("{list} entry \"{entry}\" {reason}");
        if !entry.is_ascii() {
            return Err(invalid(
                "must be ASCII; write an international domain in punycode (xn--)",
            ));
        }
        if entry.is_empty()
            || entry.contains("://")
            || entry.contains(|c: char| c.is_whitespace() || c.is_control())
        {
            return Err(invalid(SHAPE));
        }
        let (rest, any_port) = match entry.strip_suffix(":*") {
            Some(rest) => (rest, true),
            None => (entry, false),
        };
        let (host, port) = split_port(rest).map_err(&invalid)?;
        let port = match (port, any_port) {
            (Some(_), true) => return Err(invalid(SHAPE)),
            (Some(0), false) => return Err(invalid("has an invalid port")),
            (Some(port), false) => Some(port),
            (None, true) => None,
            (None, false) => return Err(invalid("needs a port, or * for any port")),
        };
        let host = normalize(host);
        let target = match host.split_once('/') {
            Some((address, prefix)) => {
                let address: IpAddr = address.parse().map_err(|_| invalid(SHAPE))?;
                let bits = if address.is_ipv4() { 32 } else { 128 };
                let prefix = prefix
                    .parse::<u8>()
                    .ok()
                    .filter(|&prefix| prefix <= bits)
                    .ok_or_else(|| invalid("has an invalid network prefix"))?;
                match address.to_canonical() {
                    // Connect addresses are canonicalised too, so an IPv4-mapped
                    // network is the IPv4 network in its last 32 bits; a shorter
                    // prefix would span IPv6 addresses as well.
                    IpAddr::V4(v4) if address.is_ipv6() => match prefix.checked_sub(96) {
                        Some(prefix) => Target::Network(IpAddr::V4(v4), prefix),
                        None => {
                            return Err(invalid(
                                "has a network prefix below 96 for an IPv4-mapped address",
                            ));
                        }
                    },
                    canonical => Target::Network(canonical, prefix),
                }
            }
            None => match host.parse::<IpAddr>() {
                Ok(address) => {
                    let address = address.to_canonical();
                    Target::Network(address, if address.is_ipv4() { 32 } else { 128 })
                }
                Err(_) if is_hostname(&host) => Target::Name(host),
                Err(_) => return Err(invalid(SHAPE)),
            },
        };
        Ok(Self { target, port })
    }

    fn port_matches(&self, address: SocketAddr) -> bool {
        self.port.is_none_or(|port| port == address.port())
    }

    fn is_name(&self) -> bool {
        matches!(self.target, Target::Name(_))
    }
}

/// Not empty, no wildcard, and not something libc reads as an IPv4 address
/// although Rust does not: `127.1`, `10.0.1` or `0x7f.1`, whose last label is
/// a number.
fn is_hostname(host: &str) -> bool {
    let last = host.rsplit('.').next().unwrap_or_default();
    !host.is_empty()
        && !host.contains('*')
        && !last.bytes().all(|b| b.is_ascii_digit())
        && !last.starts_with("0x")
}

fn contains(network: IpAddr, prefix: u8, ip: IpAddr) -> bool {
    // Saturating, so a prefix beyond the width can only narrow the match.
    let mask = |bits: u32| {
        u128::MAX
            .checked_shl(bits.saturating_sub(u32::from(prefix)))
            .unwrap_or(0)
    };
    match (network, ip) {
        (IpAddr::V4(network), IpAddr::V4(ip)) => {
            let mask = mask(32) as u32;
            u32::from(network) & mask == u32::from(ip) & mask
        }
        (IpAddr::V6(network), IpAddr::V6(ip)) => {
            let mask = mask(128);
            u128::from(network) & mask == u128::from(ip) & mask
        }
        _ => false,
    }
}

/// The addresses `name` resolves to, canonicalised as connect addresses are.
fn lookup(name: &str) -> Vec<IpAddr> {
    (name, 0)
        .to_socket_addrs()
        .map(|addresses| {
            addresses
                .map(|address| address.ip().to_canonical())
                .collect()
        })
        .unwrap_or_default()
}

/// Resolves on tokio's blocking pool of the runtime the check is polled on.
/// Never block_on here: the check already runs inside wasmtime-wasi's block_on.
/// A lookup that takes longer than `limit` counts as failed.
async fn resolve(name: String, limit: Option<Duration>) -> Option<Vec<IpAddr>> {
    let task = tokio::task::spawn_blocking(move || lookup(&name));
    match limit {
        Some(limit) => tokio::time::timeout(limit, task).await.ok()?.ok(),
        None => task.await.ok(),
    }
}

/// IP rules first, so a connect they allow waits for no lookup; then each
/// name rule for this port, resolved now.
async fn allowed(rules: &[Rule], address: SocketAddr, limit: Option<Duration>) -> bool {
    let ip = address.ip().to_canonical();
    let candidates = rules.iter().filter(|rule| rule.port_matches(address));
    let mut names = Vec::new();
    for rule in candidates {
        match &rule.target {
            Target::Network(network, prefix) if contains(*network, *prefix, ip) => return true,
            Target::Network(..) => {}
            Target::Name(name) if !names.contains(name) => names.push(name.clone()),
            Target::Name(_) => {}
        }
    }
    for name in names {
        if resolve(name, limit)
            .await
            .is_some_and(|addresses| addresses.contains(&ip))
        {
            return true;
        }
    }
    false
}

/// Parses the `tcpHosts` or `udpHosts` constructor argument of `Wasm\Wasi`,
/// named by `list`.
pub fn parse_hosts(hosts: &ZendHashTable, list: &str) -> PhpResult<Vec<Rule>> {
    hosts
        .values()
        .map(|entry: &Zval| {
            let text = entry.str().filter(|_| entry.is_string()).ok_or_else(|| {
                type_error(format!(
                    "{list} entries must be strings, got {}",
                    debug_type(entry)
                ))
            })?;
            Rule::parse(text, list).map_err(value_error)
        })
        .collect()
}

/// UDP rules as networks and ports, with every distinct name resolved now,
/// all at once and within one `limit` in total. This runs on the PHP thread,
/// which may already be inside a tokio runtime (PHP code in a host import),
/// where block_on would panic, so each lookup gets a plain thread. A name that
/// fails, times out or gets no thread allows nothing.
fn resolved(rules: &[Rule], limit: Option<Duration>) -> Vec<(IpAddr, u8, Option<u16>)> {
    let mut names: Vec<&str> = Vec::new();
    for rule in rules {
        if let Target::Name(name) = &rule.target
            && !names.contains(&name.as_str())
        {
            names.push(name);
        }
    }
    let pending: Vec<_> = names
        .iter()
        .map(|&name| {
            let (sender, receiver) = mpsc::channel();
            let owned = name.to_owned();
            let started = std::thread::Builder::new()
                .name("wasm-udp-lookup".into())
                .spawn(move || {
                    let _ = sender.send(lookup(&owned));
                });
            (name, started.ok().map(|_| receiver))
        })
        .collect();
    let deadline = limit.map(|limit| Instant::now() + limit);
    let mut addresses: Vec<(&str, Vec<IpAddr>)> = Vec::new();
    for (name, receiver) in pending {
        let result = receiver.and_then(|receiver| match deadline {
            Some(deadline) => receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .ok(),
            None => receiver.recv().ok(),
        });
        addresses.push((name, result.unwrap_or_default()));
    }

    let mut networks = Vec::new();
    for rule in rules {
        match &rule.target {
            Target::Network(network, prefix) => networks.push((*network, *prefix, rule.port)),
            Target::Name(name) => {
                let ips = addresses
                    .iter()
                    .find(|(resolved, _)| resolved == name)
                    .map(|(_, ips)| ips.as_slice())
                    .unwrap_or_default();
                for &ip in ips {
                    networks.push((ip, if ip.is_ipv4() { 32 } else { 128 }, rule.port));
                }
            }
        }
    }
    networks
}

/// Whether a UDP datagram to or from `address` is allowed.
fn udp_allows(networks: &[(IpAddr, u8, Option<u16>)], address: SocketAddr) -> bool {
    let ip = address.ip().to_canonical();
    networks.iter().any(|&(network, prefix, port)| {
        port.is_none_or(|port| port == address.port()) && contains(network, prefix, ip)
    })
}

/// Lets the guest connect to what `tcp` allows and exchange datagrams with what
/// `udp` allows; a protocol without a list stays off. Lookups are only turned
/// on with a name rule, because while on the guest can look up any name. The
/// host's own lookups for name rules wait at most `lookup_limit`.
pub fn allow(
    builder: &mut WasiCtxBuilder,
    tcp: Option<Vec<Rule>>,
    udp: Option<Vec<Rule>>,
    lookup_limit: Option<Duration>,
) {
    let names = tcp.iter().chain(udp.iter()).flatten().any(Rule::is_name);
    builder
        .allow_tcp(tcp.is_some())
        .allow_udp(udp.is_some())
        .allow_ip_name_lookup(names);
    let tcp: Arc<[Rule]> = tcp.unwrap_or_default().into();
    let udp: Arc<[(IpAddr, u8, Option<u16>)]> =
        resolved(&udp.unwrap_or_default(), lookup_limit).into();
    builder.socket_addr_check(move |address, use_| {
        let tcp = tcp.clone();
        let udp = udp.clone();
        Box::pin(async move {
            match use_ {
                // Every connect and send binds to the wildcard address first,
                // which an explicit bind to it cannot be told apart from.
                SocketAddrUse::TcpBind | SocketAddrUse::UdpBind => {
                    address.ip().is_unspecified() && address.port() == 0
                }
                SocketAddrUse::TcpConnect => allowed(&tcp, address, lookup_limit).await,
                SocketAddrUse::UdpSend | SocketAddrUse::UdpReceive => udp_allows(&udp, address),
                _ => false,
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_addresses_and_networks() {
        let rule = |entry| Rule::parse(entry, "tcpHosts").unwrap();
        assert_eq!(
            rule("DB.internal.:5432").target,
            Target::Name("db.internal".into())
        );
        assert_eq!(rule("10.0.0.0/8:*").port, None);
        assert_eq!(
            rule("[::ffff:127.0.0.1]:80").target,
            Target::Network("127.0.0.1".parse().unwrap(), 32)
        );
        assert_eq!(
            rule("[fd00::/8]:1").target,
            Target::Network("fd00::".parse().unwrap(), 8)
        );
    }

    #[test]
    fn a_mapped_network_becomes_its_ipv4_network() {
        assert_eq!(
            Rule::parse("[::ffff:10.0.0.0/104]:1", "tcpHosts")
                .unwrap()
                .target,
            Target::Network("10.0.0.0".parse().unwrap(), 8)
        );
        assert!(
            Rule::parse("[::ffff:0:0/64]:1", "tcpHosts")
                .unwrap_err()
                .contains("prefix")
        );
    }

    #[test]
    fn numbers_libc_would_read_as_addresses_are_not_names() {
        for entry in ["127.1:80", "10.0.1:80", "0x7f.1:80", "1:80", "a\0b:80"] {
            assert!(Rule::parse(entry, "tcpHosts").is_err(), "{entry}");
        }
        assert!(Rule::parse("db1.internal:80", "tcpHosts").is_ok());
    }

    #[test]
    fn a_prefix_beyond_the_width_never_widens_a_match() {
        let ip = |text: &str| text.parse::<IpAddr>().unwrap();
        assert!(!contains(ip("10.0.0.0"), 104, ip("11.0.0.1")));
    }

    #[test]
    fn udp_allows_by_network_and_port() {
        let ip = |text: &str| text.parse::<IpAddr>().unwrap();
        let networks = [(ip("127.0.0.1"), 32, Some(8125)), (ip("fd00::"), 8, None)];
        let address = |text: &str| text.parse::<SocketAddr>().unwrap();
        assert!(udp_allows(&networks, address("127.0.0.1:8125")));
        assert!(udp_allows(&networks, address("[::ffff:127.0.0.1]:8125")));
        assert!(!udp_allows(&networks, address("127.0.0.1:8126")));
        assert!(udp_allows(&networks, address("[fd12::1]:53")));
        assert!(!udp_allows(&networks, address("[fe80::1]:53")));
    }

    #[test]
    fn networks_contain_their_addresses() {
        let ip = |text: &str| text.parse::<IpAddr>().unwrap();
        assert!(contains(ip("10.0.0.0"), 8, ip("10.200.3.4")));
        assert!(!contains(ip("10.0.0.0"), 8, ip("11.0.0.1")));
        assert!(contains(ip("0.0.0.0"), 0, ip("8.8.8.8")));
        assert!(contains(ip("fd00::"), 8, ip("fdff::1")));
        assert!(!contains(ip("fd00::"), 8, ip("fe00::1")));
        assert!(!contains(ip("10.0.0.0"), 8, ip("::1")));
    }
}
