//! An async component for the extension's tests: async imports and exports,
//! streams and futures. See wit/async.wit.

wit_bindgen::generate!({ world: "async-demo", path: "wit" });

struct Component;

impl Guest for Component {
    async fn run(n: u32) -> u32 {
        slow(n).await + 1
    }

    async fn both() -> u32 {
        let (a, b) = futures::join!(slow(1), slow(2));
        a + b
    }

    async fn count_up(n: u32) -> wit_bindgen::StreamReader<u32> {
        let (mut tx, rx) = wit_stream::new::<u32>();
        wit_bindgen::spawn_local(async move {
            for i in 0..n {
                if !tx.write_all(vec![i]).await.is_empty() {
                    return;
                }
            }
        });
        rx
    }

    async fn bytes(n: u32) -> wit_bindgen::StreamReader<u8> {
        let (mut tx, rx) = wit_stream::new::<u8>();
        wit_bindgen::spawn_local(async move {
            let mut left = n as usize;
            while left > 0 {
                let chunk = left.min(3);
                if !tx.write_all(vec![b'a'; chunk]).await.is_empty() {
                    return;
                }
                left -= chunk;
            }
        });
        rx
    }

    async fn words() -> wit_bindgen::StreamReader<String> {
        let (mut tx, rx) = wit_stream::new::<String>();
        wit_bindgen::spawn_local(async move {
            for word in ["alpha", "beta", "gamma"] {
                if !tx.write_all(vec![word.to_string()]).await.is_empty() {
                    return;
                }
            }
        });
        rx
    }

    async fn endless() -> wit_bindgen::StreamReader<u8> {
        let (mut tx, rx) = wit_stream::new::<u8>();
        wit_bindgen::spawn_local(async move {
            loop {
                if !tx.write_all(vec![b'x'; 16]).await.is_empty() {
                    return;
                }
            }
        });
        rx
    }

    async fn stuck() -> wit_bindgen::StreamReader<u8> {
        let (tx, rx) = wit_stream::new::<u8>();
        std::mem::forget(tx);
        rx
    }

    async fn points() -> wit_bindgen::StreamReader<Point> {
        let (mut tx, rx) = wit_stream::new::<Point>();
        wit_bindgen::spawn_local(async move {
            let _ = tx.write_all(vec![Point { x: 1, y: 2 }]).await;
        });
        rx
    }

    async fn fire(n: u32) -> wit_bindgen::FutureReader<u32> {
        let (tx, rx) = wit_future::new::<u32>(|| 0);
        wit_bindgen::spawn_local(async move {
            let _ = tx.write(slow(n).await).await;
        });
        rx
    }

    async fn crash_later() -> wit_bindgen::StreamReader<u32> {
        let (mut tx, rx) = wit_stream::new::<u32>();
        wit_bindgen::spawn_local(async move {
            let _ = tx.write_all(vec![0]).await;
            core::arch::wasm32::unreachable();
        });
        rx
    }

    async fn ticker(n: u32) -> wit_bindgen::StreamReader<u32> {
        let (mut tx, rx) = wit_stream::new::<u32>();
        wit_bindgen::spawn_local(async move {
            for i in 0..n {
                if !tx.write_all(vec![slow(i).await]).await.is_empty() {
                    return;
                }
            }
        });
        rx
    }

    async fn rest_sum(mut s: wit_bindgen::StreamReader<u32>) -> wit_bindgen::FutureReader<u64> {
        let (tx, rx) = wit_future::new::<u64>(|| 0);
        wit_bindgen::spawn_local(async move {
            let mut total = 0u64;
            while let Some(n) = s.next().await {
                total += u64::from(n);
            }
            let _ = tx.write(total).await;
        });
        rx
    }

    async fn first(mut s: wit_bindgen::StreamReader<u32>) -> u32 {
        s.next().await.unwrap_or(0)
    }

    async fn sum(mut s: wit_bindgen::StreamReader<u32>) -> u64 {
        let mut total = 0u64;
        while let Some(n) = s.next().await {
            total += u64::from(n);
        }
        total
    }

    fn sync_sum(mut s: wit_bindgen::StreamReader<u32>) -> u64 {
        wit_bindgen::block_on(async move {
            let mut total = 0u64;
            while let Some(n) = s.next().await {
                total += u64::from(n);
            }
            total
        })
    }

    async fn race(
        mut a: wit_bindgen::StreamReader<u32>,
        mut b: wit_bindgen::StreamReader<u32>,
    ) -> u32 {
        let first = futures::future::select(Box::pin(a.next()), Box::pin(b.next())).await;
        match first {
            futures::future::Either::Left((n, _)) | futures::future::Either::Right((n, _)) => {
                n.unwrap_or(0)
            }
        }
    }

    async fn length(s: wit_bindgen::StreamReader<u8>) -> u64 {
        s.collect().await.len() as u64
    }

    async fn greet_later(name: String) -> wit_bindgen::FutureReader<String> {
        let (tx, rx) = wit_future::new::<String>(String::new);
        wit_bindgen::spawn_local(async move {
            let _ = tx.write(format!("hello, {name}")).await;
        });
        rx
    }

    async fn await_value(f: wit_bindgen::FutureReader<String>) -> String {
        format!("{}!", f.await)
    }
}

export!(Component);
