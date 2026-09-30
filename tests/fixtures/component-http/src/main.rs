//! A WASI preview2 command for the extension's HTTP tests: `get <url>` prints
//! the status and body, or `error: <code>`.

use wasi::http::outgoing_handler;
use wasi::http::types::{Fields, OutgoingRequest, Scheme};
use wasi::io::streams::StreamError;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let url = args.get(2).expect("usage: get <url>");
    match get(url) {
        Ok((status, body)) => print!("{status}\n{body}"),
        Err(error) => print!("error: {error}"),
    }
}

fn get(url: &str) -> Result<(u16, String), String> {
    let (scheme, rest) = url.split_once("://").ok_or("the url needs a scheme")?;
    let (authority, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, "/"),
    };
    let request = OutgoingRequest::new(Fields::new());
    let scheme = if scheme == "https" { Scheme::Https } else { Scheme::Http };
    request.set_scheme(Some(&scheme)).map_err(|()| "scheme")?;
    request.set_authority(Some(authority)).map_err(|()| "authority")?;
    request.set_path_with_query(Some(path)).map_err(|()| "path")?;

    let response = outgoing_handler::handle(request, None).map_err(|code| format!("{code:?}"))?;
    response.subscribe().block();
    let response = response
        .get()
        .ok_or("no response")?
        .map_err(|()| "response taken twice")?
        .map_err(|code| format!("{code:?}"))?;

    let status = response.status();
    let body = response.consume().map_err(|()| "body")?;
    let mut bytes = Vec::new();
    {
        let stream = body.stream().map_err(|()| "stream")?;
        loop {
            match stream.blocking_read(4096) {
                Ok(chunk) => bytes.extend(chunk),
                Err(StreamError::Closed) => break,
                Err(error) => return Err(format!("{error:?}")),
            }
        }
    }
    Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
}
