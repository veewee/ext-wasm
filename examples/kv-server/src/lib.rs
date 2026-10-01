//! A small Redis compatible key-value server as an async WebAssembly component.
//!
//! PHP owns the sockets: it accepts a connection and passes what the client
//! sends as the input stream of `serve`, and writes the returned stream back
//! to the client. The component only speaks the protocol (RESP2). The data
//! lives in the component instance, so it outlives a single connection.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

wit_bindgen::generate!({ world: "server", path: "wit" });

use exports::example::kv_server::connection::Guest;

struct Entry {
    value: Vec<u8>,
    expires: Option<Instant>,
}

impl Entry {
    fn alive(&self) -> bool {
        self.expires.is_none_or(|at| Instant::now() < at)
    }
}

thread_local! {
    static DB: RefCell<HashMap<Vec<u8>, Entry>> = RefCell::new(HashMap::new());
}

struct Component;

impl Guest for Component {
    async fn serve(mut input: wit_bindgen::StreamReader<u8>) -> wit_bindgen::StreamReader<u8> {
        let (mut output, replies) = wit_stream::new::<u8>();
        wit_bindgen::spawn_local(async move {
            let mut buffer = Vec::new();
            loop {
                let (result, chunk) = input.read(Vec::with_capacity(16 * 1024)).await;
                if chunk.is_empty() && !matches!(result, wit_bindgen::StreamResult::Complete(_)) {
                    return;
                }
                buffer.extend_from_slice(&chunk);
                // One write for all commands that arrived together, so a
                // pipelined batch gets its replies in one chunk.
                let mut out = Vec::new();
                let open = handle(&mut buffer, &mut out);
                if !out.is_empty() && !output.write_all(out).await.is_empty() {
                    return;
                }
                if !open {
                    return;
                }
            }
        });
        replies
    }
}

export!(Component);

/// Runs every complete command in `buffer` and removes it, leaving a
/// command that has not fully arrived for the next chunk. Returns false
/// when the connection should close.
fn handle(buffer: &mut Vec<u8>, out: &mut Vec<u8>) -> bool {
    let mut used = 0;
    let mut open = true;
    while open {
        match parse(&buffer[used..]) {
            Ok(Some((args, len))) => {
                used += len;
                if !args.is_empty() {
                    open = execute(&args, out);
                }
            }
            Ok(None) => break,
            Err(message) => {
                error(out, &format!("Protocol error: {message}"));
                open = false;
            }
        }
    }
    buffer.drain(..used);
    open
}

type Parsed = Result<Option<(Vec<Vec<u8>>, usize)>, &'static str>;

/// Parses one command from the start of `buf`: a RESP array of bulk strings,
/// or an inline command as typed in telnet. `Ok(None)` means more bytes are
/// needed.
fn parse(buf: &[u8]) -> Parsed {
    let Some(end) = line_end(buf, 0) else {
        return Ok(None);
    };
    if buf.first() != Some(&b'*') {
        let line = buf[..end].strip_suffix(b"\r").unwrap_or(&buf[..end]);
        let args = line
            .split(|b| b.is_ascii_whitespace())
            .filter(|word| !word.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        return Ok(Some((args, end + 1)));
    }
    let count = number(&buf[1..end]).ok_or("invalid multibulk length")?;
    let mut pos = end + 1;
    let mut args = Vec::new();
    for _ in 0..count {
        let Some(end) = line_end(buf, pos) else {
            return Ok(None);
        };
        if buf[pos] != b'$' {
            return Err("expected '$'");
        }
        let len = number(&buf[pos + 1..end]).ok_or("invalid bulk length")?;
        let start = end + 1;
        if buf.len() < start + len + 2 {
            return Ok(None);
        }
        args.push(buf[start..start + len].to_vec());
        pos = start + len + 2;
    }
    Ok(Some((args, pos)))
}

fn line_end(buf: &[u8], from: usize) -> Option<usize> {
    buf.get(from..)?
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| from + i)
}

/// A length from a RESP header line, which ends in "\r".
fn number(digits: &[u8]) -> Option<usize> {
    let digits = digits.strip_suffix(b"\r")?;
    let n: usize = std::str::from_utf8(digits).ok()?.parse().ok()?;
    (n <= 512 * 1024 * 1024).then_some(n)
}

/// Runs one command and appends its reply. Returns false after QUIT.
fn execute(args: &[Vec<u8>], out: &mut Vec<u8>) -> bool {
    let name = String::from_utf8_lossy(&args[0]).to_ascii_uppercase();
    let rest = &args[1..];
    let arity_ok = match name.as_str() {
        "PING" => rest.len() <= 1,
        "ECHO" | "GET" | "INCR" | "KEYS" => rest.len() == 1,
        "SET" => rest.len() >= 2,
        "DEL" | "EXISTS" => !rest.is_empty(),
        "DBSIZE" | "QUIT" => rest.is_empty(),
        "COMMAND" => true,
        _ => {
            error(
                out,
                &format!("unknown command '{}'", String::from_utf8_lossy(&args[0])),
            );
            return true;
        }
    };
    if !arity_ok {
        error(
            out,
            &format!(
                "wrong number of arguments for '{}' command",
                name.to_ascii_lowercase()
            ),
        );
        return true;
    }
    DB.with_borrow_mut(|db| match name.as_str() {
        "PING" => match rest.first() {
            Some(message) => bulk(out, Some(message)),
            None => simple(out, "PONG"),
        },
        "ECHO" => bulk(out, Some(&rest[0])),
        "SET" => set(db, rest, out),
        "GET" => bulk(out, live(db, &rest[0]).map(|entry| entry.value.as_slice())),
        "DEL" => integer(
            out,
            rest.iter()
                .filter(|key| db.remove(*key).is_some_and(|entry| entry.alive()))
                .count() as i64,
        ),
        "EXISTS" => integer(
            out,
            rest.iter().filter(|key| live(db, key).is_some()).count() as i64,
        ),
        "INCR" => incr(db, &rest[0], out),
        "KEYS" => {
            db.retain(|_, entry| entry.alive());
            let keys: Vec<&Vec<u8>> = db.keys().filter(|key| glob(&rest[0], key)).collect();
            out.extend_from_slice(format!("*{}\r\n", keys.len()).as_bytes());
            for key in keys {
                bulk(out, Some(key));
            }
        }
        "DBSIZE" => {
            db.retain(|_, entry| entry.alive());
            integer(out, db.len() as i64);
        }
        // redis-cli asks for the command docs when it starts; an empty list
        // makes it go without hints.
        "COMMAND" => out.extend_from_slice(b"*0\r\n"),
        _ => simple(out, "OK"), // QUIT
    });
    name != "QUIT"
}

/// The entry for `key`, unless it expired; an expired one is removed.
fn live<'a>(db: &'a mut HashMap<Vec<u8>, Entry>, key: &[u8]) -> Option<&'a mut Entry> {
    if db.get(key).is_some_and(|entry| !entry.alive()) {
        db.remove(key);
    }
    db.get_mut(key)
}

fn set(db: &mut HashMap<Vec<u8>, Entry>, args: &[Vec<u8>], out: &mut Vec<u8>) {
    let expires = match &args[2..] {
        [] => None,
        [option, seconds] if option.eq_ignore_ascii_case(b"EX") => {
            match std::str::from_utf8(seconds)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
            {
                Some(0) => return error(out, "invalid expire time in 'set' command"),
                Some(seconds) => Some(Instant::now() + Duration::from_secs(seconds)),
                None => return error(out, "value is not an integer or out of range"),
            }
        }
        _ => return error(out, "syntax error"),
    };
    db.insert(
        args[0].clone(),
        Entry {
            value: args[1].clone(),
            expires,
        },
    );
    simple(out, "OK");
}

fn incr(db: &mut HashMap<Vec<u8>, Entry>, key: &[u8], out: &mut Vec<u8>) {
    let current = match live(db, key) {
        Some(entry) => match std::str::from_utf8(&entry.value)
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
        {
            Some(n) => n,
            None => return error(out, "value is not an integer or out of range"),
        },
        None => 0,
    };
    let Some(next) = current.checked_add(1) else {
        return error(out, "increment or decrement would overflow");
    };
    // INCR keeps the expiry of the key, as Redis does.
    let expires = live(db, key).and_then(|entry| entry.expires);
    db.insert(
        key.to_vec(),
        Entry {
            value: next.to_string().into_bytes(),
            expires,
        },
    );
    integer(out, next);
}

/// Redis style glob with `*` and `?`.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            glob(&pattern[1..], text) || (!text.is_empty() && glob(pattern, &text[1..]))
        }
        (Some(b'?'), Some(_)) => glob(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => glob(&pattern[1..], &text[1..]),
        _ => false,
    }
}

fn simple(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(format!("+{text}\r\n").as_bytes());
}

fn error(out: &mut Vec<u8>, message: &str) {
    out.extend_from_slice(format!("-ERR {message}\r\n").as_bytes());
}

fn integer(out: &mut Vec<u8>, n: i64) {
    out.extend_from_slice(format!(":{n}\r\n").as_bytes());
}

fn bulk(out: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(value) => {
            out.extend_from_slice(format!("${}\r\n", value.len()).as_bytes());
            out.extend_from_slice(value);
            out.extend_from_slice(b"\r\n");
        }
        None => out.extend_from_slice(b"$-1\r\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (mut buffer, mut out) = (input.to_vec(), Vec::new());
        handle(&mut buffer, &mut out);
        (out, buffer)
    }

    #[test]
    fn keeps_a_partial_command_for_the_next_chunk() {
        assert_eq!(
            run(b"*1\r\n$4\r\nPING\r\n*2\r\n$4\r\nECHO\r\n$5\r\nhel"),
            (
                b"+PONG\r\n".to_vec(),
                b"*2\r\n$4\r\nECHO\r\n$5\r\nhel".to_vec()
            )
        );
    }

    #[test]
    fn answers_inline_commands() {
        assert_eq!(
            run(b"ECHO hi\r\nPING"),
            (b"$2\r\nhi\r\n".to_vec(), b"PING".to_vec())
        );
    }

    #[test]
    fn matches_globs() {
        assert!(glob(b"user:*", b"user:42"));
        assert!(glob(b"?a*", b"bar"));
        assert!(!glob(b"user:?", b"user:42"));
    }
}
