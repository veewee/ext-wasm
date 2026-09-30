//! HTML sanitizing with ammonia and HTML rewriting with lol-html, compiled to a
//! plain wasm module.
//!
//! Strings cross the boundary like in the rust-markdown example: the host asks
//! `alloc` for room and writes the input there, and each export hands back the
//! output's pointer and length packed into one u64 (pointer in the high half).
//! `rewrite` can fail on bad rules, so its output starts with a status byte:
//! 0 for HTML, 1 for an error message.

use std::borrow::Cow;
use std::collections::BTreeMap;

use lol_html::html_content::Element;
use lol_html::{ElementContentHandlers, HandlerResult, RewriteStrSettings, Selector, rewrite_str};
use serde::Deserialize;

#[derive(Deserialize)]
struct Rule {
    selector: String,
    #[serde(default)]
    set: BTreeMap<String, String>,
    #[serde(default)]
    remove: bool,
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from `alloc` or an export and be freed only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Keeps text and safe markup, and drops scripts, event handlers and
/// `javascript:` links, with ammonia's default allow list.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 written by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sanitize(ptr: *const u8, len: usize) -> u64 {
    pack(ammonia::clean(&input(ptr, len)).into_bytes())
}

/// Applies rules like `[{"selector": "img", "set": {"loading": "lazy"}}]` or
/// `[{"selector": ".ad", "remove": true}]` to a document.
///
/// # Safety
/// Both pointers must point to their length in UTF-8 bytes written by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rewrite(
    html_ptr: *const u8,
    html_len: usize,
    rules_ptr: *const u8,
    rules_len: usize,
) -> u64 {
    let (status, text) = match apply(&input(html_ptr, html_len), &input(rules_ptr, rules_len)) {
        Ok(html) => (0, html),
        Err(error) => (1, error),
    };
    let mut out = vec![status];
    out.extend_from_slice(text.as_bytes());
    pack(out)
}

fn apply(html: &str, rules: &str) -> Result<String, String> {
    let rules: Vec<Rule> =
        serde_json::from_str(rules).map_err(|e| format!("invalid rules: {e}"))?;
    let mut settings = RewriteStrSettings::new();
    for rule in rules {
        let selector: Selector = rule
            .selector
            .parse()
            .map_err(|e| format!("invalid selector {:?}: {e}", rule.selector))?;
        let handler = move |element: &mut Element<'_, '_>| -> HandlerResult {
            if rule.remove {
                element.remove();
            }
            for (name, value) in &rule.set {
                element.set_attribute(name, value)?;
            }
            Ok(())
        };
        settings = settings.append_element_content_handler((
            Cow::Owned(selector),
            ElementContentHandlers::default().element(handler),
        ));
    }
    rewrite_str(html, settings).map_err(|e| e.to_string())
}

fn input<'a>(ptr: *const u8, len: usize) -> Cow<'a, str> {
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
}

fn pack(bytes: Vec<u8>) -> u64 {
    let out = bytes.into_boxed_slice();
    let (len, ptr) = (out.len(), Box::into_raw(out) as *mut u8);
    ((ptr as u64) << 32) | len as u64
}
