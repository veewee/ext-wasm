//! CommonMark to HTML with pulldown-cmark, compiled to a WebAssembly component.
//!
//! wit/markdown.wit declares the interface, and wit-bindgen generates the code
//! that moves strings across the boundary, so this file only renders.

use pulldown_cmark::{Options, Parser, html};

wit_bindgen::generate!({ world: "renderer", path: "wit" });

struct Renderer;

impl exports::docs::markdown::render::Guest for Renderer {
    fn render(markdown: String) -> String {
        let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        let mut out = String::new();
        html::push_html(&mut out, Parser::new_ext(&markdown, options));
        out
    }
}

export!(Renderer);
