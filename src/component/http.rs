//! Outgoing wasi:http for components, restricted to the hosts PHP allows.

use std::future::Future;
use std::time::Duration;

use ext_php_rs::exception::PhpResult;
use ext_php_rs::types::{ZendHashTable, Zval};
use wasmtime_wasi_http::{Error, RequestOptions, WasiHttpCtx, WasiHttpHooks};

use crate::error::{type_error, value_error};
use crate::value::debug_type;

/// The wasi:http state of a component store given a `Wasm\Wasi` with `httpHosts`.
pub struct WasiHttp {
    pub ctx: WasiHttpCtx,
    pub hooks: Hooks,
}

impl WasiHttp {
    pub fn new(rules: Vec<HostRule>, timeout: Option<Duration>) -> Self {
        Self {
            ctx: WasiHttpCtx::new(),
            hooks: Hooks { rules, timeout },
        }
    }
}

/// One `httpHosts` entry: a host, optionally with a port, or `*.` and a domain
/// for its subdomains.
#[derive(Clone, Debug, PartialEq)]
pub struct HostRule {
    host: String,
    subdomains: bool,
    port: Option<u16>,
}

impl HostRule {
    pub fn parse(entry: &str) -> Result<Self, String> {
        let invalid = |reason: &str| format!("httpHosts entry \"{entry}\" {reason}");
        if !entry.is_ascii() {
            return Err(invalid(
                "must be ASCII; write an international domain in punycode (xn--)",
            ));
        }
        if entry.is_empty()
            || entry.contains("://")
            || entry.contains('/')
            || entry.contains(char::is_whitespace)
        {
            return Err(invalid(
                "must be a host, host:port or *.domain, without scheme or path",
            ));
        }
        let (rest, subdomains) = match entry.strip_prefix("*.") {
            Some(domain) => (domain, true),
            None => (entry, false),
        };
        let (host, port) = split_port(rest).map_err(&invalid)?;
        let host = normalize(host);
        if host.is_empty() || host.contains('*') {
            return Err(invalid(
                "must be a host, host:port or *.domain, without scheme or path",
            ));
        }
        Ok(Self {
            host,
            subdomains,
            port,
        })
    }

    fn allows(&self, host: &str, port: u16) -> bool {
        let host_matches = if self.subdomains {
            host.strip_suffix(self.host.as_str())
                .is_some_and(|prefix| prefix.len() > 1 && prefix.ends_with('.'))
        } else {
            host == self.host
        };
        host_matches && self.port.is_none_or(|allowed| allowed == port)
    }
}

/// Splits `host:port`, keeping the colons of a bracketed IPv6 literal.
pub(crate) fn split_port(entry: &str) -> Result<(&str, Option<u16>), &'static str> {
    let (host, port) = if let Some(rest) = entry.strip_prefix('[') {
        let (address, after) = rest.split_once(']').ok_or("has an unclosed [")?;
        if !after.is_empty() && !after.starts_with(':') {
            return Err("has something after the ] that is not a port");
        }
        (address, after.strip_prefix(':'))
    } else if entry.matches(':').count() == 1 {
        let (host, port) = entry.split_once(':').expect("one colon");
        (host, Some(port))
    } else {
        // Zero colons, or a bare IPv6 address.
        (entry, None)
    };
    match port {
        None => Ok((host, None)),
        Some(port) => port
            .parse::<u16>()
            .map(|port| (host, Some(port)))
            .map_err(|_| "has an invalid port"),
    }
}

/// Lowercase, without the brackets of an IPv6 literal or a trailing dot.
pub(crate) fn normalize(host: &str) -> String {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

/// Parses the `httpHosts` constructor argument of `Wasm\Wasi`.
pub fn parse_hosts(hosts: &ZendHashTable) -> PhpResult<Vec<HostRule>> {
    hosts
        .values()
        .map(|entry: &Zval| {
            let text = entry.str().filter(|_| entry.is_string()).ok_or_else(|| {
                type_error(format!(
                    "httpHosts entries must be strings, got {}",
                    debug_type(entry)
                ))
            })?;
            HostRule::parse(text).map_err(value_error)
        })
        .collect()
}

pub struct Hooks {
    rules: Vec<HostRule>,
    /// PHP's default_socket_timeout, the longest any timeout of a request may be.
    timeout: Option<Duration>,
}

impl Hooks {
    fn allows(&self, uri: &http::Uri) -> bool {
        let Some(host) = uri.host() else {
            return false;
        };
        let port = uri
            .port_u16()
            .unwrap_or(if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            });
        let host = normalize(host);
        self.rules.iter().any(|rule| rule.allows(&host, port))
    }

    fn capped(&self, options: Option<RequestOptions>) -> Option<RequestOptions> {
        let Some(limit) = self.timeout else {
            return options;
        };
        let cap = |asked: Option<Duration>| Some(asked.map_or(limit, |asked| asked.min(limit)));
        let options = options.unwrap_or_default();
        Some(RequestOptions {
            connect_timeout: cap(options.connect_timeout),
            first_byte_timeout: cap(options.first_byte_timeout),
            between_bytes_timeout: cap(options.between_bytes_timeout),
        })
    }
}

type ErrorFuture = Box<dyn Future<Output = Result<(), Error>> + Send>;

impl WasiHttpHooks for Hooks {
    fn send_request(
        &mut self,
        request: http::Request<wasmtime_wasi_http::WasiBody>,
        options: Option<RequestOptions>,
        fut: ErrorFuture,
    ) -> Box<
        dyn Future<
                Output = Result<(http::Response<wasmtime_wasi_http::WasiBody>, ErrorFuture), Error>,
            > + Send,
    > {
        drop(fut);
        if !self.allows(request.uri()) {
            return Box::new(async { Err(Error::HttpRequestDenied) });
        }
        let options = self.capped(options);
        // wasmtime-wasi-http puts no timeout on the TLS handshake, so the
        // whole setup until the response headers is bounded by the connect
        // and first-byte timeouts together.
        let setup = options
            .and_then(|options| Some(options.connect_timeout? + options.first_byte_timeout?));
        Box::new(async move {
            use http_body_util::BodyExt;

            let sending = wasmtime_wasi_http::default_send_request(request, options);
            let (response, io) = match setup {
                Some(limit) => tokio::time::timeout(limit, sending)
                    .await
                    .map_err(|_| Error::ConnectionTimeout)??,
                None => sending.await?,
            };
            Ok((
                response.map(BodyExt::boxed_unsync),
                Box::new(io) as ErrorFuture,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::HostRule;

    #[test]
    fn rules_match_hosts_ports_and_subdomains() {
        let rule = |entry| HostRule::parse(entry).unwrap();
        assert!(rule("example.com").allows("example.com", 443));
        assert!(!rule("example.com").allows("api.example.com", 443));
        assert!(rule("example.com:8080").allows("example.com", 8080));
        assert!(!rule("example.com:8080").allows("example.com", 80));
        assert!(rule("*.example.com").allows("api.example.com", 80));
        assert!(!rule("*.example.com").allows("example.com", 80));
        assert!(!rule("*.example.com").allows("badexample.com", 80));
        assert!(rule("EXAMPLE.com.").allows("example.com", 80));
        assert!(rule("[::1]:8080").allows("::1", 8080));
        assert!(rule("::1").allows("::1", 80));
        assert!(HostRule::parse("http://example.com").is_err());
        assert!(HostRule::parse("example.com/api").is_err());
        assert!(HostRule::parse("example.com:http").is_err());
        assert!(HostRule::parse("").is_err());
        assert!(HostRule::parse("bücher.example").is_err());
        assert!(HostRule::parse("[::1]garbage").is_err());
        assert!(rule("Example.COM").allows("example.com", 80));
    }
}
