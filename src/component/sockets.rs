//! Outgoing TCP for components, restricted to the destinations PHP allows.
//!
//! wasmtime-wasi asks before each socket operation with only an IP address
//! and a port, since the guest resolves names itself. A name rule is therefore
//! checked by resolving it on the host when the guest connects.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use ext_php_rs::exception::PhpResult;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::sockets::SocketAddrUse;

use crate::component::http::{normalize, split_port};
use crate::error::{type_error, value_error};
use crate::value::debug_type;

/// One `tcpHosts` entry: a name, an address or a network, and a port or any.
#[derive(Clone, Debug, PartialEq)]
pub struct TcpRule {
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

impl TcpRule {
    pub fn parse(entry: &str) -> Result<Self, String> {
        let invalid = |reason: &str| format!("tcpHosts entry \"{entry}\" {reason}");
        if !entry.is_ascii() {
            return Err(invalid(
                "must be ASCII; write an international domain in punycode (xn--)",
            ));
        }
        if entry.is_empty() || entry.contains("://") || entry.contains(char::is_whitespace) {
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
                Target::Network(address.to_canonical(), prefix)
            }
            None => match host.parse::<IpAddr>() {
                Ok(address) => {
                    let address = address.to_canonical();
                    Target::Network(address, if address.is_ipv4() { 32 } else { 128 })
                }
                Err(_) if !host.is_empty() && !host.contains('*') => Target::Name(host),
                Err(_) => return Err(invalid(SHAPE)),
            },
        };
        Ok(Self { target, port })
    }

    /// Whether a connect to `address` is allowed, resolving a name rule now.
    async fn allows(&self, address: SocketAddr) -> bool {
        if self.port.is_some_and(|port| port != address.port()) {
            return false;
        }
        let ip = address.ip().to_canonical();
        match &self.target {
            Target::Network(network, prefix) => contains(*network, *prefix, ip),
            Target::Name(name) => resolve(name.clone())
                .await
                .is_some_and(|addresses| addresses.contains(&ip)),
        }
    }

    fn is_name(&self) -> bool {
        matches!(self.target, Target::Name(_))
    }
}

fn contains(network: IpAddr, prefix: u8, ip: IpAddr) -> bool {
    let mask = |bits: u32| u128::MAX.checked_shl(bits - u32::from(prefix)).unwrap_or(0);
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

/// Resolves on tokio's blocking pool of the runtime the check is polled on.
/// Never block_on here: the check already runs inside wasmtime-wasi's block_on.
async fn resolve(name: String) -> Option<Vec<IpAddr>> {
    let lookup = tokio::task::spawn_blocking(move || {
        (name.as_str(), 0).to_socket_addrs().map(|addresses| {
            addresses
                .map(|address| address.ip().to_canonical())
                .collect()
        })
    });
    lookup.await.ok()?.ok()
}

/// Parses the `tcpHosts` constructor argument of `Wasm\Wasi`.
pub fn parse_hosts(hosts: &ZendHashTable) -> PhpResult<Vec<TcpRule>> {
    hosts
        .values()
        .map(|entry: &Zval| {
            let text = entry.str().filter(|_| entry.is_string()).ok_or_else(|| {
                type_error(format!(
                    "tcpHosts entries must be strings, got {}",
                    debug_type(entry)
                ))
            })?;
            TcpRule::parse(text).map_err(value_error)
        })
        .collect()
}

/// Lets the guest connect to what `rules` allow. Lookups are only turned on
/// with a name rule, because while on the guest can look up any name.
pub fn allow(builder: &mut WasiCtxBuilder, rules: Vec<TcpRule>) {
    let rules: Arc<[TcpRule]> = rules.into();
    builder
        .allow_tcp(true)
        .allow_ip_name_lookup(rules.iter().any(TcpRule::is_name));
    builder.socket_addr_check(move |address, use_| {
        let rules = rules.clone();
        Box::pin(async move {
            match use_ {
                // Every connect binds to the wildcard address first, which an
                // explicit bind to it cannot be told apart from.
                SocketAddrUse::TcpBind => address.ip().is_unspecified() && address.port() == 0,
                SocketAddrUse::TcpConnect => {
                    for rule in rules.iter() {
                        if rule.allows(address).await {
                            return true;
                        }
                    }
                    false
                }
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
        let rule = |entry| TcpRule::parse(entry).unwrap();
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
