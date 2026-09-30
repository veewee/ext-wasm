//! A `wasi:http/proxy` component that also imports `later`, so PHP can give
//! it a `Wasm\Suspending`. It answers every request with 200 and a body that
//! is the decimal result of `later(1)`.

use std::io::Write as _;

use wasi::http::types::{Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam};

wit_bindgen::generate!({
    inline: "package docs:later; world later { import later: func(n: u32) -> u32; }",
});

wasi::http::proxy::export!(Handler);

struct Handler;

impl wasi::exports::http::incoming_handler::Guest for Handler {
    fn handle(_request: IncomingRequest, response_out: ResponseOutparam) {
        let answer = later(1);
        let response = OutgoingResponse::new(Fields::new());
        let outgoing = response.body().unwrap();
        ResponseOutparam::set(response_out, Ok(response));
        let mut out = outgoing.write().unwrap();
        out.write_all(answer.to_string().as_bytes()).unwrap();
        out.flush().unwrap();
        drop(out);
        OutgoingBody::finish(outgoing, None).unwrap();
    }
}
