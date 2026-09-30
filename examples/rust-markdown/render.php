<?php

// Renders Markdown to HTML with Rust's pulldown-cmark, compiled to wasm.
// Usage: php examples/rust-markdown/render.php < README.md

declare(strict_types=1);

require __DIR__ . '/Markdown.php';

$markdown = stream_isatty(STDIN) ? "# Hello\n\nRendered by **Rust**, called from *PHP*.\n\n- [x] tables\n- [x] ~~strikethrough~~\n" : stream_get_contents(STDIN);

echo (new Markdown())->toHtml($markdown);
