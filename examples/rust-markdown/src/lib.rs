//! CommonMark to HTML with pulldown-cmark, compiled to a plain wasm module.
//!
//! Strings cross the boundary through linear memory: the host asks `alloc` for
//! room, writes the input there, and `render` hands back the output's pointer
//! and length packed into one u64 (pointer in the high half). The host frees
//! both buffers with `dealloc` when it is done.

use pulldown_cmark::{Options, Parser, html};

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from `alloc` or `render` and be freed only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 written by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn render(ptr: *const u8, len: usize) -> u64 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let markdown = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(&markdown, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS));
    let out = out.into_bytes().into_boxed_slice();
    let (len, ptr) = (out.len(), Box::into_raw(out) as *mut u8);
    ((ptr as u64) << 32) | len as u64
}
