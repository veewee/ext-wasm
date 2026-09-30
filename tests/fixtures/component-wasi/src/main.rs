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
        _ => {}
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
