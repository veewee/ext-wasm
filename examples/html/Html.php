<?php

declare(strict_types=1);

/**
 * HTML sanitizing with ammonia and HTML rewriting with Cloudflare's lol-html,
 * both Rust crates compiled into one wasm module by src/lib.rs.
 *
 * Strings pass through the module's memory as in the rust-markdown example.
 * rewrite() prefixes its output with a status byte, 1 meaning the rest is an
 * error message.
 */
final class Html
{
    private readonly Wasm\Exports $wasm;

    public function __construct(string $wasmFile = __DIR__ . '/html.wasm')
    {
        $this->wasm = (new Wasm\Instance(Wasm\Module::fromFile($wasmFile)))->exports;
    }

    /** Keeps safe markup and drops scripts, event handlers and javascript: links. */
    public function sanitize(string $html): string
    {
        $input = $this->put($html);
        try {
            return $this->take($this->wasm->sanitize($input, strlen($html)));
        } finally {
            $this->wasm->dealloc($input, strlen($html));
        }
    }

    /**
     * @param list<array{selector: string, set?: array<string, string>, remove?: bool}> $rules
     */
    public function rewrite(string $html, array $rules): string
    {
        $json = json_encode($rules, JSON_THROW_ON_ERROR);
        [$input, $rulesInput] = [$this->put($html), $this->put($json)];
        try {
            $output = $this->take($this->wasm->rewrite($input, strlen($html), $rulesInput, strlen($json)));
        } finally {
            $this->wasm->dealloc($input, strlen($html));
            $this->wasm->dealloc($rulesInput, strlen($json));
        }

        if ($output[0] === "\1") {
            throw new InvalidArgumentException(substr($output, 1));
        }

        return substr($output, 1);
    }

    private function put(string $bytes): int
    {
        $pointer = $this->wasm->alloc(strlen($bytes));
        $this->wasm->memory->write($pointer, $bytes);

        return $pointer;
    }

    private function take(int $packed): string
    {
        // i64 results arrive signed, so the pointer is masked back to 32 bits.
        [$pointer, $length] = [($packed >> 32) & 0xFFFFFFFF, $packed & 0xFFFFFFFF];
        $bytes = $this->wasm->memory->read($pointer, $length);
        $this->wasm->dealloc($pointer, $length);

        return $bytes;
    }
}
