<?php

declare(strict_types=1);

/**
 * Typst templates to PDF, with the Typst compiler built to wasm by src/lib.rs.
 *
 * The data is passed as JSON and the template reads it with json("data.json").
 * The output starts with a status byte, 1 meaning the rest is an error message.
 */
final class Typst
{
    private readonly Wasm\Exports $wasm;

    public function __construct(string $wasmFile = __DIR__ . '/dist/typst.wasm')
    {
        $this->wasm = (new Wasm\Instance(Wasm\Module::fromFile($wasmFile)))->exports;
    }

    /** @throws RuntimeException with Typst's error messages when the template does not compile */
    public function pdf(string $template, mixed $data = []): string
    {
        $json = json_encode($data, JSON_THROW_ON_ERROR);
        [$templateInput, $dataInput] = [$this->put($template), $this->put($json)];
        try {
            $packed = $this->wasm->compile($templateInput, strlen($template), $dataInput, strlen($json));
        } finally {
            $this->wasm->dealloc($templateInput, strlen($template));
            $this->wasm->dealloc($dataInput, strlen($json));
        }

        // i64 results arrive signed, so the pointer is masked back to 32 bits.
        [$pointer, $length] = [($packed >> 32) & 0xFFFFFFFF, $packed & 0xFFFFFFFF];
        $output = $this->wasm->memory->read($pointer, $length);
        $this->wasm->dealloc($pointer, $length);

        if ($output[0] === "\1") {
            throw new RuntimeException(substr($output, 1));
        }

        return substr($output, 1);
    }

    private function put(string $bytes): int
    {
        $pointer = $this->wasm->alloc(strlen($bytes));
        $this->wasm->memory->write($pointer, $bytes);

        return $pointer;
    }
}
