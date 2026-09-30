<?php

declare(strict_types=1);

/**
 * CommonMark to HTML, rendered by the Rust crate in src/lib.rs.
 *
 * The module exports its memory and three functions. Strings go in by asking
 * alloc() for room and writing the bytes there; render() answers with the
 * output's pointer and length packed into one integer.
 */
final class Markdown
{
    private readonly Wasm\Exports $wasm;

    public function __construct(string $wasmFile = __DIR__ . '/markdown.wasm')
    {
        $this->wasm = (new Wasm\Instance(new Wasm\Module(file_get_contents($wasmFile))))->exports;
    }

    public function toHtml(string $markdown): string
    {
        $input = $this->wasm->alloc(strlen($markdown));
        $this->wasm->memory->write($input, $markdown);

        $packed = $this->wasm->render($input, strlen($markdown));
        // i64 results arrive signed, so the pointer is masked back to 32 bits.
        [$output, $length] = [($packed >> 32) & 0xFFFFFFFF, $packed & 0xFFFFFFFF];
        $html = $this->wasm->memory->read($output, $length);

        $this->wasm->dealloc($input, strlen($markdown));
        $this->wasm->dealloc($output, $length);

        return $html;
    }
}
