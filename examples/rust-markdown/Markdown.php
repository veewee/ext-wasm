<?php

declare(strict_types=1);

/**
 * CommonMark to HTML, rendered by the Rust component in src/lib.rs.
 *
 * wit/markdown.wit declares render(markdown: string) -> string, so PHP passes
 * and receives plain strings. Rust's standard library imports WASI, which the
 * Wasi object provides with nothing of the host visible.
 */
final class Markdown
{
    private readonly Wasm\Component\Exports $render;

    public function __construct(string $wasmFile = __DIR__ . '/markdown.wasm')
    {
        $instance = new Wasm\Component\Instance(
            Wasm\Component\Component::fromFile($wasmFile),
            wasi: new Wasm\Wasi(),
        );
        $this->render = $instance->exports->get('docs:markdown/render');
    }

    public function toHtml(string $markdown): string
    {
        return $this->render->render($markdown);
    }
}
