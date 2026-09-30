//! Link previews and a reader mode, compiled to a WebAssembly component.
//!
//! The component fetches pages itself through wasi:http, so the host decides
//! which hosts it may reach. It offers its two functions as typed exports
//! (wit/pages.wit) and over HTTP as a wasi:http/incoming-handler.

use std::io::Write as _;
use std::time::{Duration, Instant};

use scraper::{Html, Selector};
use url::Url;
use wasi::http::outgoing_handler;
use wasi::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingRequest, OutgoingResponse, ResponseOutparam, Scheme,
};
use wasi::io::streams::StreamError;

wit_bindgen::generate!({ world: "link-preview", path: "wit" });

use exports::docs::link_preview::pages::{Guest, PagePreview};

struct Component;

impl Guest for Component {
    fn preview(url: String) -> Result<PagePreview, String> {
        let (url, html) = fetch(&url)?;
        Ok(preview_of(&url, &Html::parse_document(&html)))
    }

    fn read(url: String) -> Result<String, String> {
        let (url, html) = fetch(&url)?;
        read_of(&url, &Html::parse_document(&html))
    }
}

export!(Component);
wasi::http::proxy::export!(Component);

/// The largest page this reads, so a huge page cannot fill the memory.
const MAX_BODY: usize = 5 << 20;
const MAX_REDIRECTS: usize = 5;
/// For the whole fetch, redirects included, so a server that sends a byte
/// at a time cannot keep a request open.
const DEADLINE: Duration = Duration::from_secs(20);
/// How deeply elements may nest. Parsing and converting recurse over the
/// tree, and a page nested a few hundred levels deeper exhausts the stack,
/// which traps the whole instance; real pages stay far below this.
const MAX_DEPTH: usize = 256;

/// Fetches an HTML page, following redirects. Every hop is a request of its
/// own, which the host checks against its list of allowed hosts again.
fn fetch(url: &str) -> Result<(Url, String), String> {
    let mut url = Url::parse(url).map_err(|err| format!("\"{url}\" is not a URL: {err}"))?;
    let started = Instant::now();
    let in_time = |url: &Url| {
        if started.elapsed() > DEADLINE {
            return Err(format!("fetching {url} took longer than {} seconds", DEADLINE.as_secs()));
        }
        Ok(())
    };
    for _ in 0..=MAX_REDIRECTS {
        in_time(&url)?;
        let response = get(&url)?;
        let status = response.status();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .first()
                .map(|value| String::from_utf8_lossy(value).into_owned())
        };
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let location = header("location").ok_or(format!("{url} redirects without a location"))?;
            url = url.join(&location).map_err(|err| format!("bad redirect to {location}: {err}"))?;
            continue;
        }
        if status != 200 {
            return Err(format!("{url} answered with status {status}"));
        }
        let content_type = header("content-type").unwrap_or_default();
        if !content_type.contains("html") {
            return Err(format!("{url} is not an HTML page but {content_type}"));
        }
        let body = response.consume().map_err(|()| "the body was taken")?;
        let mut bytes = Vec::new();
        {
            let stream = body.stream().map_err(|()| "the body stream was taken")?;
            loop {
                match stream.blocking_read(64 * 1024) {
                    Ok(chunk) => bytes.extend(chunk),
                    Err(StreamError::Closed) => break,
                    Err(err) => return Err(format!("reading {url} failed: {err:?}")),
                }
                if bytes.len() > MAX_BODY {
                    return Err(format!("{url} is larger than {MAX_BODY} bytes"));
                }
                in_time(&url)?;
            }
        }
        let html = String::from_utf8_lossy(&bytes).into_owned();
        if nesting_of(&html) > MAX_DEPTH {
            return Err(format!("{url} nests its elements more than {MAX_DEPTH} levels deep"));
        }
        return Ok((url, html));
    }
    Err(format!("more than {MAX_REDIRECTS} redirects"))
}

/// How deeply the elements of `html` nest, estimated from its tags before
/// parsing, since the parse itself is what a deep page makes slow. It keeps
/// a stack of open tags as the parser does: a closing tag pops up to its
/// opening one and is ignored without one, and void elements, tags that
/// close themselves and tags whose end is optional do not nest.
fn nesting_of(html: &str) -> usize {
    const FLAT: &[&str] = &[
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
        "track", "wbr", "p", "li", "dt", "dd", "tr", "td", "th", "option", "optgroup", "thead", "tbody",
        "tfoot", "colgroup", "caption", "rt", "rp", "html", "head", "body",
    ];
    let bytes = html.as_bytes();
    let mut open: Vec<String> = Vec::new();
    let mut deepest = 0;
    let mut at = 0;
    while let Some(offset) = html[at..].find('<') {
        at += offset + 1;
        let closing = bytes.get(at) == Some(&b'/');
        let start = if closing { at + 1 } else { at };
        let end = html[start..]
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .map_or(html.len(), |length| start + length);
        if end == start {
            continue;
        }
        let name = html[start..end].to_ascii_lowercase();
        let tag_end = html[end..].find('>').map_or(html.len(), |length| end + length);
        if closing {
            if let Some(position) = open.iter().rposition(|tag| *tag == name) {
                open.truncate(position);
            }
        } else if name == "script" || name == "style" {
            // Their text may hold `<` that is no tag.
            let close = format!("</{name}");
            at = html[tag_end..].to_ascii_lowercase().find(&close).map_or(html.len(), |length| tag_end + length);
            continue;
        } else if !FLAT.contains(&name.as_str()) && !html[..tag_end].ends_with('/') {
            open.push(name);
            deepest = deepest.max(open.len());
        }
        at = tag_end.min(html.len());
    }
    deepest
}

fn get(url: &Url) -> Result<wasi::http::types::IncomingResponse, String> {
    let scheme = match url.scheme() {
        "https" => Scheme::Https,
        "http" => Scheme::Http,
        other => return Err(format!("{other} URLs are not supported")),
    };
    let authority = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
        None => url.host_str().unwrap_or_default().to_string(),
    };
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_string(),
    };
    let headers = Fields::new();
    let _ = headers.set("user-agent", &[b"ext-wasm link-preview example".to_vec()]);
    let _ = headers.set("accept", &[b"text/html".to_vec()]);
    let request = OutgoingRequest::new(headers);
    request.set_scheme(Some(&scheme)).map_err(|()| "invalid scheme")?;
    request.set_authority(Some(&authority)).map_err(|()| "invalid host")?;
    request.set_path_with_query(Some(&path)).map_err(|()| "invalid path")?;

    let pending = outgoing_handler::handle(request, None).map_err(|code| format!("fetching {url} failed: {code:?}"))?;
    pending.subscribe().block();
    pending
        .get()
        .ok_or("no response")?
        .map_err(|()| "the response was taken")?
        .map_err(|code| format!("fetching {url} failed: {code:?}"))
}

fn preview_of(url: &Url, page: &Html) -> PagePreview {
    let meta = |names: &[&str]| {
        names.iter().find_map(|name| {
            let selector = Selector::parse(&format!("meta[property=\"{name}\"], meta[name=\"{name}\"]")).ok()?;
            page.select(&selector)
                .find_map(|element| element.value().attr("content"))
                .map(|content| content.trim().to_string())
                .filter(|content| !content.is_empty())
        })
    };
    PagePreview {
        url: url.to_string(),
        title: meta(&["og:title", "twitter:title"]).or_else(|| title_of(page)),
        description: meta(&["og:description", "twitter:description", "description"]),
        // Only web URLs, so a `javascript:` image cannot reach a page that shows it.
        image: meta(&["og:image", "twitter:image"])
            .and_then(|image| url.join(&image).ok())
            .filter(|image| matches!(image.scheme(), "http" | "https"))
            .map(String::from),
        site_name: meta(&["og:site_name"]),
    }
}

fn title_of(page: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;
    page.select(&selector)
        .next()
        .map(|title| title.text().collect::<String>().trim().to_string())
        .filter(|title| !title.is_empty())
}

/// The main content as Markdown: the page's `article` or `main`, otherwise
/// its body, without navigation, scripts and forms. A plain heuristic, not a
/// full readability algorithm.
fn read_of(url: &Url, page: &Html) -> Result<String, String> {
    let main = ["article", "main", "[role=main]", "body"]
        .iter()
        .filter_map(|selector| Selector::parse(selector).ok())
        .find_map(|selector| page.select(&selector).next())
        .map(|element| element.html())
        .ok_or("the page has no body")?;
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "nav", "header", "footer", "aside", "form", "noscript", "svg", "iframe"])
        .build();
    let markdown = converter.convert(&main).map_err(|err| format!("converting {url} failed: {err}"))?;
    Ok(match title_of(page) {
        Some(title) if !markdown.trim_start().starts_with('#') => format!("# {title}\n\n{}", markdown.trim()),
        _ => markdown.trim().to_string(),
    })
}

impl wasi::exports::http::incoming_handler::Guest for Component {
    /// `GET /preview?url=...` answers JSON, `GET /read?url=...` Markdown.
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let target = request.path_with_query().unwrap_or_default();
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        let url = url::form_urlencoded::parse(query.as_bytes())
            .find(|(name, _)| name == "url")
            .map(|(_, value)| value.into_owned());
        let is_get = matches!(request.method(), wasi::http::types::Method::Get);
        let (status, content_type, body) = match (path, url) {
            ("/preview" | "/read", _) if !is_get => (405, "text/plain", "only GET".to_string()),
            ("/preview" | "/read", None) => (400, "text/plain", "add ?url=https://...".to_string()),
            ("/preview", Some(url)) => match Component::preview(url) {
                Ok(preview) => (200, "application/json", preview_json(&preview)),
                Err(error) => (502, "text/plain", error),
            },
            ("/read", Some(url)) => match Component::read(url) {
                Ok(markdown) => (200, "text/markdown; charset=utf-8", markdown),
                Err(error) => (502, "text/plain", error),
            },
            _ => (404, "text/plain", "try /preview?url= or /read?url=".to_string()),
        };
        respond(response_out, status, content_type, body.as_bytes());
    }
}

fn preview_json(preview: &PagePreview) -> String {
    serde_json::json!({
        "url": preview.url,
        "title": preview.title,
        "description": preview.description,
        "image": preview.image,
        "siteName": preview.site_name,
    })
    .to_string()
}

fn respond(response_out: ResponseOutparam, status: u16, content_type: &str, body: &[u8]) {
    let headers = Fields::new();
    let _ = headers.set("content-type", &[content_type.as_bytes().to_vec()]);
    let response = OutgoingResponse::new(headers);
    let _ = response.set_status_code(status);
    let Ok(outgoing) = response.body() else {
        return;
    };
    ResponseOutparam::set(response_out, Ok(response));
    if let Ok(mut out) = outgoing.write() {
        let _ = out.write_all(body);
        let _ = out.flush();
    }
    let _ = OutgoingBody::finish(outgoing, None);
}
