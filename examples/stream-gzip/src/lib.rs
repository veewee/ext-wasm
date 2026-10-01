//! Streaming gzip compression as an async WebAssembly component.
//!
//! The component reads its input and writes its output as streams, so
//! neither side needs the whole file in memory: PHP hands over chunks as the
//! component asks for them and receives compressed chunks as they are made.

use std::io::Write as _;

use flate2::Compression;
use flate2::write::GzEncoder;

wit_bindgen::generate!({ world: "compressor", path: "wit" });

use exports::docs::stream_gzip::gzip::Guest;

struct Component;

impl Guest for Component {
    async fn compress(mut input: wit_bindgen::StreamReader<u8>, level: u32) -> wit_bindgen::StreamReader<u8> {
        let (mut output, compressed) = wit_stream::new::<u8>();
        wit_bindgen::spawn_local(async move {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level.min(9)));
            loop {
                let (result, chunk) = input.read(Vec::with_capacity(64 * 1024)).await;
                if chunk.is_empty() && !matches!(result, wit_bindgen::StreamResult::Complete(_)) {
                    break;
                }
                if encoder.write_all(&chunk).is_err() {
                    return;
                }
                // Hands over what the encoder produced so far.
                let ready = std::mem::take(encoder.get_mut());
                if !ready.is_empty() && !output.write_all(ready).await.is_empty() {
                    return;
                }
            }
            if let Ok(rest) = encoder.finish() {
                let _ = output.write_all(rest).await;
            }
        });
        compressed
    }
}

export!(Component);
