//! A WASI preview2 command for the extension's tests. The first argument
//! picks what it does.

use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("print") => {
            println!("args={}", args[2..].join(" "));
            println!("greeting={}", std::env::var("GREETING").unwrap_or_default());
        }
        Some("stdin") => {
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input).unwrap();
            std::io::stdout().write_all(&input).unwrap();
        }
        Some("read") => match std::fs::read_to_string("/in/data.txt") {
            Ok(text) => print!("{text}"),
            Err(err) => print!("error: {err}"),
        },
        Some("write") => match std::fs::write("/out/result.txt", "written") {
            Ok(()) => print!("ok"),
            Err(err) => print!("error: {err}"),
        },
        Some("stream") => print!("{}", read_through_stream()),
        Some("flood") => {
            let bytes: usize = args[2].parse().unwrap();
            print!("{}", "x".repeat(bytes));
        }
        Some("exit") => std::process::exit(args[2].parse().unwrap()),
        Some("tcp") => print!("{}", exchange(&args[2], &args[3])),
        Some("udp") => print!("{}", datagram(&args[2], &args[3])),
        Some("listen") => match std::net::TcpListener::bind(args[2].as_str()) {
            Ok(_) => print!("listening"),
            Err(err) => print!("error: {:?}: {err}", err.kind()),
        },
        _ => {}
    }
}

/// Connects to `address`, sends `message` and returns the reply, or the error
/// with its kind, so tests can tell a refused permission from a failed lookup.
fn exchange(address: &str, message: &str) -> String {
    use std::net::TcpStream;
    let result = (|| {
        let mut stream = TcpStream::connect(address)?;
        stream.write_all(message.as_bytes())?;
        stream.shutdown(std::net::Shutdown::Write)?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply)?;
        Ok::<_, std::io::Error>(reply)
    })();
    match result {
        Ok(reply) => format!("reply: {reply}"),
        Err(err) => format!("error: {:?}: {err}", err.kind()),
    }
}

/// Sends `message` to `address` and returns the first reply within two
/// seconds, or the error with its kind. It resolves the name itself and uses
/// an address of the socket's family, since std's send_to tries only the first
/// address, and it polls nonblocking, since read timeouts fail on wasip2.
fn datagram(address: &str, message: &str) -> String {
    use std::net::{ToSocketAddrs, UdpSocket};
    use std::time::{Duration, Instant};
    let result = (|| {
        let addresses: Vec<_> = address.to_socket_addrs()?.collect();
        let target = addresses
            .iter()
            .find(|address| address.is_ipv4())
            .or(addresses.first())
            .copied()
            .ok_or_else(|| std::io::Error::other("no address"))?;
        let socket = UdpSocket::bind(if target.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })?;
        socket.set_nonblocking(true)?;
        socket.send_to(message.as_bytes(), target)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut reply = [0; 512];
        loop {
            match socket.recv_from(&mut reply) {
                Ok((read, _)) => {
                    return Ok(String::from_utf8_lossy(&reply[..read]).into_owned());
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "no reply"));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => return Err(err),
            }
        }
    })();
    match result {
        Ok(reply) => format!("reply: {reply}"),
        Err(err) => format!("error: {:?}: {err}", err.kind()),
    }
}

/// Reads /in/data.txt through the non-blocking `read` of a file stream after
/// waiting on its pollable, the path wasmtime serves from tokio's blocking pool.
fn read_through_stream() -> String {
    use wasi::filesystem::preopens::get_directories;
    use wasi::filesystem::types::{DescriptorFlags, OpenFlags, PathFlags};
    use wasi::io::streams::StreamError;

    let (dir, _) = get_directories()
        .into_iter()
        .find(|(_, path)| path == "/in")
        .expect("/in is preopened");
    let file = dir
        .open_at(PathFlags::empty(), "data.txt", OpenFlags::empty(), DescriptorFlags::READ)
        .expect("data.txt opens");
    let stream = file.read_via_stream(0).expect("a read stream");
    let mut text = Vec::new();
    loop {
        stream.subscribe().block();
        match stream.read(4096) {
            Ok(chunk) => text.extend(chunk),
            Err(StreamError::Closed) => break,
            Err(err) => return format!("error: {err:?}"),
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}
