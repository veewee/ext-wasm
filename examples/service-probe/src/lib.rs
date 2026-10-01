//! Works out what listens on a TCP port, compiled to a WebAssembly component.
//!
//! Plain Rust std::net: on wasm32-wasip2 it goes through wasi:sockets, so the
//! host decides which addresses the component may connect to. Services that
//! speak first (SSH, SMTP, FTP, POP3, IMAP, MySQL) are read from their banner;
//! for the others the component sends one small request per connection and
//! reads the answer.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

wit_bindgen::generate!({ world: "service-probe", path: "wit" });

use exports::example::service_probe::probe::{Guest, Report};

struct Component;

impl Guest for Component {
    fn identify(target: String, timeout_ms: u32) -> Result<Report, String> {
        identify(&target, Duration::from_millis(u64::from(timeout_ms.max(1))))
    }
}

export!(Component);

/// The most of an answer that is read; banners and status lines are short.
const READ_LIMIT: usize = 1024;

fn identify(target: &str, timeout: Duration) -> Result<Report, String> {
    let host = target.rsplit_once(':').map_or(target, |(host, _)| host);
    let addresses: Vec<SocketAddr> = target
        .to_socket_addrs()
        .map_err(|err| format!("cannot resolve {target}: {err}"))?
        .collect();
    let (address, mut stream, connected_in) = connect_any(&addresses, timeout)?;
    let report = |service: &str, detail: String| Report {
        address: address.to_string(),
        service: service.into(),
        detail,
        connect_ms: u32::try_from(connected_in.as_millis()).unwrap_or(u32::MAX),
    };

    let banner = read_some(&mut stream, timeout);
    if let Some((service, detail)) = from_banner(&banner) {
        return Ok(report(service, detail));
    }
    drop(stream);

    let probes: [Vec<u8>; 3] = [
        b"PING\r\n".to_vec(),
        // PostgreSQL's SSLRequest: length 8, then the code 80877103.
        vec![0, 0, 0, 8, 0x04, 0xd2, 0x16, 0x2f],
        format!("HEAD / HTTP/1.0\r\nHost: {host}\r\nUser-Agent: service-probe\r\n\r\n")
            .into_bytes(),
    ];
    for request in &probes {
        let Ok(mut stream) = connect(&address, timeout) else {
            continue;
        };
        if stream.write_all(request).is_err() {
            continue;
        }
        let reply = read_some(&mut stream, timeout);
        if let Some((service, detail)) = from_reply(request, &reply) {
            return Ok(report(service, detail));
        }
    }
    let detail = if banner.is_empty() {
        "accepts connections but answered none of the probes".into()
    } else {
        printable(&banner)
    };
    Ok(report("unknown", detail))
}

/// Tries each address in turn, as `TcpStream::connect` does, but with a timeout.
fn connect_any(
    addresses: &[SocketAddr],
    timeout: Duration,
) -> Result<(SocketAddr, TcpStream, Duration), String> {
    let mut last = String::from("no addresses");
    for &address in addresses {
        let started = Instant::now();
        match connect(&address, timeout) {
            Ok(stream) => return Ok((address, stream, started.elapsed())),
            Err(err) => last = format!("{address}: {err}"),
        }
    }
    Err(format!("cannot connect, {last}"))
}

/// `connect_timeout`, checked for a connect that failed: on wasm32-wasip2 it
/// returns Ok for a refused connection, and the error only shows afterwards.
fn connect(address: &SocketAddr, timeout: Duration) -> std::io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(address, timeout)?;
    if let Some(err) = stream.take_error()? {
        return Err(err);
    }
    stream.peer_addr()?;
    Ok(stream)
}

/// Whatever arrives within `timeout`, up to READ_LIMIT bytes; nothing for a
/// service that waits for the client to speak.
fn read_some(stream: &mut TcpStream, timeout: Duration) -> Vec<u8> {
    let _ = stream.set_read_timeout(Some(timeout));
    let mut buffer = vec![0; READ_LIMIT];
    match stream.read(&mut buffer) {
        Ok(read) => buffer.truncate(read),
        Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            buffer.clear()
        }
        Err(_) => buffer.clear(),
    }
    buffer
}

fn from_banner(banner: &[u8]) -> Option<(&'static str, String)> {
    let line = first_line(banner);
    if banner.starts_with(b"SSH-") {
        return Some(("SSH", line));
    }
    if banner.starts_with(b"220") {
        let service = if line.to_ascii_lowercase().contains("ftp") {
            "FTP"
        } else {
            "SMTP"
        };
        return Some((service, line));
    }
    if banner.starts_with(b"+OK") {
        return Some(("POP3", line));
    }
    if banner.starts_with(b"* OK") {
        return Some(("IMAP", line));
    }
    mysql_greeting(banner)
}

/// MySQL and MariaDB greet with a packet: a 3-byte length, sequence 0, then
/// protocol 10 and the server version, or an error packet (0xff).
fn mysql_greeting(packet: &[u8]) -> Option<(&'static str, String)> {
    if packet.len() < 6 || packet[3] != 0 {
        return None;
    }
    let body = &packet[4..];
    match body[0] {
        0x0a => {
            let version = &body[1..];
            let version =
                String::from_utf8_lossy(&version[..version.iter().position(|&b| b == 0)?]);
            let service = if version.contains("MariaDB") {
                "MariaDB"
            } else {
                "MySQL"
            };
            Some((service, version.into_owned()))
        }
        0xff if body.len() > 3 => Some(("MySQL", printable(&body[3..]))),
        _ => None,
    }
}

fn from_reply(request: &[u8], reply: &[u8]) -> Option<(&'static str, String)> {
    if reply.starts_with(b"HTTP/") {
        let mut lines = String::from_utf8_lossy(reply)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .into_iter();
        let status = lines.next().unwrap_or_default();
        let server = lines
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("server")
                    .then(|| value.trim().to_owned())
            })
            .map_or(String::new(), |server| format!(", {server}"));
        return Some(("HTTP", format!("{status}{server}")));
    }
    // A TLS alert record: the service wanted a handshake first.
    if reply.len() >= 2 && reply[0] == 0x15 && reply[1] == 0x03 {
        return Some((
            "TLS",
            "expects a TLS handshake, contents not inspected".into(),
        ));
    }
    match request {
        b"PING\r\n" if reply.starts_with(b"+PONG") => Some(("Redis", "answers PING".into())),
        b"PING\r\n" if reply.starts_with(b"-NOAUTH") => {
            Some(("Redis", "requires a password".into()))
        }
        [0, 0, 0, 8, ..] if reply == b"S" => Some(("PostgreSQL", "accepts TLS".into())),
        [0, 0, 0, 8, ..] if reply == b"N" => Some(("PostgreSQL", "without TLS".into())),
        _ => None,
    }
}

fn first_line(bytes: &[u8]) -> String {
    printable(bytes.split(|&b| b == b'\n').next().unwrap_or_default())
}

fn printable(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect::<String>()
        .trim()
        .to_owned()
}
