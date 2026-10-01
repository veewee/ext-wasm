//! A `wasi:http/proxy` component for the extension's tests. It answers every
//! request with 201, an `x-echo` header naming the method and path, and a body
//! that repeats the request body. Paths change what it does:
//! `/large` answers with 3 MiB, `/error` answers with an error code, and
//! `/silent` never sets a response.

use std::io::{Read as _, Write as _};

use wasi::http::types::{ErrorCode, Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam};

wasi::http::proxy::export!(Handler);

struct Handler;

impl wasi::exports::http::incoming_handler::Guest for Handler {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let path = request.path_with_query().unwrap_or_default();
        match path.as_str() {
            "/error" => return ResponseOutparam::set(response_out, Err(ErrorCode::HttpRequestDenied)),
            "/silent" => return,
            _ => {}
        }
        let method = format!("{:?}", request.method());
        let greeting = request
            .headers()
            .get("x-greeting")
            .first()
            .map(|value| String::from_utf8_lossy(value).into_owned())
            .unwrap_or_default();
        let mut body = Vec::new();
        if let Ok(incoming) = request.consume()
            && let Ok(mut stream) = incoming.stream()
        {
            let _ = stream.read_to_end(&mut body);
        }

        let headers = Fields::new();
        headers
            .set("x-echo", &[format!("{method} {path}").into_bytes()])
            .unwrap();
        headers.set("x-greeting", &[greeting.into_bytes()]).unwrap();
        let response = OutgoingResponse::new(headers);
        response.set_status_code(201).unwrap();
        let outgoing = response.body().unwrap();
        ResponseOutparam::set(response_out, Ok(response));

        let mut out = outgoing.write().unwrap();
        if path == "/large" {
            let chunk = vec![b'x'; 64 * 1024];
            for _ in 0..48 {
                out.write_all(&chunk).unwrap();
            }
        } else {
            out.write_all(&body).unwrap();
        }
        out.flush().unwrap();
        drop(out);
        OutgoingBody::finish(outgoing, None).unwrap();
    }
}
