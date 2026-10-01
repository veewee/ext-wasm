//! Incoming HTTP: PHP hands a request to a component's
//! `wasi:http/incoming-handler` and gets its response back, as plain value
//! objects rather than a PSR-7 dependency.

use bytes::Bytes;
use ext_php_rs::binary::Binary;
use ext_php_rs::binary_slice::BinarySlice;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use http_body_util::{BodyExt, Full};
use wasmtime::component::Val;
use wasmtime_wasi_http::WasiHttpView;

use crate::error::{error, runtime_error, type_error, value_error};
use crate::store::SharedStore;
use crate::throw::call_error;
use crate::value::debug_type;

/// An HTTP request for a component, like `new Request('GET', 'https://example.com/')`.
///
/// Header names are lowercase and every name maps to a list of values.
#[php_class]
#[php(name = "Wasm\\Component\\Http\\Request")]
#[php(flags = ClassFlags::Final)]
pub struct Request {
    method: String,
    url: String,
    headers: Zval,
    body: Vec<u8>,
}

#[php_impl]
impl Request {
    /// @param array<string, string|list<string>>|null $headers
    pub fn __construct(
        method: String,
        url: String,
        headers: Option<&ZendHashTable>,
        body: Option<BinarySlice<u8>>,
    ) -> PhpResult<Self> {
        let parsed: http::Uri = url
            .parse()
            .map_err(|_| value_error(format!("\"{url}\" is not a URL")))?;
        if !matches!(parsed.scheme_str(), Some("http" | "https")) || parsed.authority().is_none() {
            return Err(value_error(format!(
                "\"{url}\" must be an absolute http or https URL"
            )));
        }
        Ok(Self {
            method: method.to_ascii_uppercase(),
            url,
            headers: normalize_headers(headers)?,
            body: body.map(|body| body.to_vec()).unwrap_or_default(),
        })
    }

    #[php(getter)]
    pub fn get_method(&self) -> String {
        self.method.clone()
    }

    #[php(getter)]
    pub fn get_url(&self) -> String {
        self.url.clone()
    }

    /// @return array<string, list<string>>
    #[php(getter)]
    pub fn get_headers(&self) -> Zval {
        self.headers.shallow_clone()
    }

    #[php(getter)]
    pub fn get_body(&self) -> Binary<u8> {
        self.body.clone().into()
    }
}

/// The HTTP response of a component.
#[php_class]
#[php(name = "Wasm\\Component\\Http\\Response")]
#[php(flags = ClassFlags::Final)]
pub struct Response {
    status: i64,
    headers: Zval,
    body: Vec<u8>,
}

#[php_impl]
impl Response {
    /// @param array<string, string|list<string>>|null $headers
    pub fn __construct(
        status: i64,
        headers: Option<&ZendHashTable>,
        body: Option<BinarySlice<u8>>,
    ) -> PhpResult<Self> {
        Ok(Self {
            status,
            headers: normalize_headers(headers)?,
            body: body.map(|body| body.to_vec()).unwrap_or_default(),
        })
    }

    #[php(getter)]
    pub fn get_status(&self) -> i64 {
        self.status
    }

    /// @return array<string, list<string>>
    #[php(getter)]
    pub fn get_headers(&self) -> Zval {
        self.headers.shallow_clone()
    }

    #[php(getter)]
    pub fn get_body(&self) -> Binary<u8> {
        self.body.clone().into()
    }
}

/// Lowercase names, each with a list of string values.
fn normalize_headers(headers: Option<&ZendHashTable>) -> PhpResult<Zval> {
    let mut normalized = ZendHashTable::new();
    for (name, value) in headers.into_iter().flat_map(ZendHashTable::iter) {
        let name = name.to_string().to_ascii_lowercase();
        let mut values = ZendHashTable::new();
        let string = |value: &Zval| -> PhpResult<String> {
            value
                .str()
                .filter(|_| value.is_string())
                .map(str::to_string)
                .ok_or_else(|| {
                    type_error(format!(
                        "header \"{name}\" must be a string or a list of strings, got {}",
                        debug_type(value)
                    ))
                })
        };
        match value.array() {
            Some(list) => {
                for item in list.values() {
                    values.push(string(item)?)?;
                }
            }
            None => values.push(string(value)?)?,
        }
        normalized.insert(name.as_str(), values)?;
    }
    Ok(normalized.into_zval(false)?)
}

/// Calls `handle` of the component's `wasi:http/incoming-handler` with `request`.
pub fn handle(
    store: &SharedStore,
    handler: wasmtime::component::Func,
    request: &Request,
) -> PhpResult<Response> {
    let uri: http::Uri = request
        .url
        .parse()
        .map_err(|_| value_error("the request URL is invalid"))?;
    use wasmtime_wasi_http::p2::bindings::http::types::Scheme;
    let scheme = if uri.scheme_str() == Some("https") {
        Scheme::Https
    } else {
        Scheme::Http
    };
    let mut builder = http::Request::builder()
        .method(request.method.as_str())
        .uri(uri);
    if let Some(headers) = request.headers.array() {
        for (name, values) in headers.iter() {
            for value in values.array().into_iter().flat_map(ZendHashTable::values) {
                builder = builder.header(name.to_string(), value.str().unwrap_or_default());
            }
        }
    }
    let body = Full::new(Bytes::from(request.body.clone()))
        .map_err(|never| -> wasmtime_wasi_http::Error { match never {} });
    let http_request = builder
        .body(body)
        .map_err(|err| value_error(format!("invalid request: {err}")))?;

    store.with(|mut ctx| {
        if ctx.data().http.is_none() {
            return Err(error(
                "handling HTTP requests needs a Wasm\\Wasi with httpHosts, an empty list is enough",
            ));
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let (incoming, outparam) = {
            let mut http = ctx.data_mut().http();
            let incoming = http
                .new_incoming_request(scheme, http_request)
                .map_err(runtime_error)?;
            let outparam = http.new_response_outparam(sender).map_err(runtime_error)?;
            (incoming, outparam)
        };
        let incoming = incoming
            .try_into_resource_any(&mut ctx)
            .map_err(runtime_error)?;
        let outparam = outparam
            .try_into_resource_any(&mut ctx)
            .map_err(runtime_error)?;

        // The body flows through a channel of a chunk or two, so it is read
        // while the component writes it, on the runtime `with` entered.
        let runtime = crate::engine::wasi_runtime();
        let collector = runtime.spawn(async move {
            let response = match receiver.await {
                Ok(Ok(response)) => response,
                Ok(Err(code)) => {
                    return Err(format!("the component answered with the error {code:?}"));
                }
                Err(_) => return Err("the component did not set a response".to_string()),
            };
            let (parts, body) = response.into_parts();
            let body = body
                .collect()
                .await
                .map_err(|err| format!("the response body failed: {err:?}"))?
                .to_bytes();
            Ok((parts, body))
        });
        if let Err(err) = crate::component::func::run(
            store,
            &mut ctx,
            handler,
            &[Val::Resource(incoming), Val::Resource(outparam)],
            &mut [],
        ) {
            collector.abort();
            return Err(call_error(&mut ctx, err));
        }
        // The component has returned, so nothing more can arrive after a
        // while; one that kept its response handle would otherwise block forever.
        let limit = crate::wasi::socket_timeout().unwrap_or(std::time::Duration::from_secs(600));
        let (parts, body) = runtime
            .block_on(async move {
                match tokio::time::timeout(limit, collector).await {
                    Ok(joined) => joined.map_err(|err| err.to_string()),
                    Err(_) => Err("the component did not finish its response in time".to_string()),
                }
            })
            .map_err(|err| runtime_error(wasmtime::Error::msg(err)))?
            .map_err(|message| runtime_error(wasmtime::Error::msg(message)))?;

        let mut headers = ZendHashTable::new();
        for name in parts.headers.keys() {
            let mut values = ZendHashTable::new();
            for value in parts.headers.get_all(name) {
                values.push(Binary::from(value.as_bytes().to_vec()))?;
            }
            headers.insert(name.as_str(), values)?;
        }
        Ok(Response {
            status: i64::from(parts.status.as_u16()),
            headers: headers.into_zval(false)?,
            body: body.to_vec(),
        })
    })
}
