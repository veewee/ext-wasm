//! Typst templates to PDF, compiled to a plain wasm module.
//!
//! The module embeds the fonts that ship with Typst and has no file system:
//! the template is the main file and the host's JSON data is the only other
//! file, readable from Typst as `json("data.json")`. Strings and bytes cross the
//! boundary as in the rust-markdown example. The output of `compile` starts with
//! a status byte: 0 when the rest is a PDF, 1 when it is an error message.

use std::sync::LazyLock;

use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_layout::PagedDocument;

static LIBRARY: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::default()));
static FONTS: LazyLock<Vec<Font>> = LazyLock::new(|| {
    typst_assets::fonts()
        .flat_map(|data| Font::iter(Bytes::new(data)))
        .collect()
});
static BOOK: LazyLock<LazyHash<FontBook>> =
    LazyLock::new(|| LazyHash::new(FontBook::from_fonts(FONTS.iter())));

fn file_id(path: &str) -> FileId {
    RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("valid path"),
    )
    .intern()
}

struct Document {
    main: Source,
    data: Bytes,
}

impl World for Document {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &BOOK
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(not_found(id))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == file_id("data.json") {
            Ok(self.data.clone())
        } else {
            Err(not_found(id))
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        FONTS.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

fn not_found(id: FileId) -> FileError {
    FileError::NotFound(id.vpath().get_without_slash().into())
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from `alloc` or `compile` and be freed only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// # Safety
/// The template must be `template_len` bytes of UTF-8 and the data
/// `data_len` bytes, both written by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn compile(
    template_ptr: *const u8,
    template_len: usize,
    data_ptr: *const u8,
    data_len: usize,
) -> u64 {
    let template =
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(template_ptr, template_len) })
            .into_owned();
    let data = Bytes::new(unsafe { std::slice::from_raw_parts(data_ptr, data_len) }.to_vec());
    let document = Document {
        main: Source::new(file_id("main.typ"), template),
        data,
    };

    let result = typst::compile::<PagedDocument>(&document)
        .output
        .and_then(|pages| typst_pdf::pdf(&pages, &Default::default()));
    let mut out = match result {
        Ok(pdf) => [vec![0], pdf].concat(),
        Err(errors) => [vec![1], describe(&document, &errors).into_bytes()].concat(),
    };
    out.shrink_to_fit();
    let out = out.into_boxed_slice();
    let (len, ptr) = (out.len(), Box::into_raw(out) as *mut u8);
    ((ptr as u64) << 32) | len as u64
}

/// Formats errors like the Typst CLI's short form: `main.typ:3: message`.
fn describe(document: &Document, errors: &[SourceDiagnostic]) -> String {
    let mut lines = Vec::new();
    for error in errors {
        let line = (error.span.id() == Some(document.main.id()))
            .then(|| document.range(error.span))
            .flatten()
            .and_then(|range| document.main.lines().byte_to_line(range.start));
        lines.push(match line {
            Some(line) => format!("main.typ:{}: {}", line + 1, error.message),
            None => error.message.to_string(),
        });
        lines.extend(error.hints.iter().map(|hint| format!("  hint: {}", hint.v)));
    }
    lines.join("\n")
}
