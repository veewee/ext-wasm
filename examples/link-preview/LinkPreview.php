<?php

declare(strict_types=1);

use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Http\Request;
use Wasm\Component\Http\Response;
use Wasm\Component\Instance;

/**
 * Link previews and a reader mode by the Rust component in src/lib.rs.
 *
 * The component fetches pages itself, through wasi:http, and reaches only
 * the hosts given here: a page on any other host, a redirect to one
 * included, fails inside the component before anything is sent.
 */
final class LinkPreview
{
    private readonly Instance $instance;
    private readonly Exports $pages;

    /** @param list<string> $allowedHosts such as 'example.com', 'localhost:8080' or '*.wikipedia.org' */
    public function __construct(array $allowedHosts, string $wasmFile = __DIR__ . '/link-preview.wasm')
    {
        $this->instance = new Instance(Component::fromFile($wasmFile), wasi: new Wasm\Wasi(httpHosts: $allowedHosts));
        $this->pages = $this->instance->exports->get('docs:link-preview/pages');
    }

    /**
     * @return array{url: string, title: ?string, description: ?string, image: ?string, siteName: ?string}
     * @throws Wasm\Exception\ComponentError with the reason as its payload
     */
    public function preview(string $url): array
    {
        return $this->pages->preview($url);
    }

    /** The main content of the page as Markdown. */
    public function read(string $url): string
    {
        return $this->pages->read($url);
    }

    /** Answers GET /preview?url= with JSON and GET /read?url= with Markdown. */
    public function handle(Request $request): Response
    {
        return $this->instance->handle($request);
    }
}
