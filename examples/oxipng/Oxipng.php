<?php

declare(strict_types=1);

/**
 * Lossless PNG optimisation with oxipng, a Rust crate compiled to wasm with
 * wasm-bindgen. This class does what wasm-bindgen's JavaScript glue does in
 * the browser: copy the bytes into the module's memory, call the export, and
 * read the result back from a return slot on wasm's own stack.
 */
final class Oxipng
{
    private readonly Wasm\Exports $wasm;

    public function __construct(string $wasmFile = __DIR__ . '/dist/oxipng.wasm')
    {
        // The import needs the exports to read the error message from memory.
        // A weak reference avoids a cycle through the store, which PHP's
        // garbage collector cannot see.
        $exports = null;
        $instance = new Wasm\Instance(Wasm\Module::fromFile($wasmFile), [
            'wbg' => [
                '__wbindgen_throw' => static function (int $pointer, int $length) use (&$exports): never {
                    throw new RuntimeException($exports->get()->memory->read($pointer, $length));
                },
            ],
        ]);
        $this->wasm = $instance->exports;
        $exports = WeakReference::create($this->wasm);
    }

    /** @param int<0, 6> $level higher is smaller and slower */
    public function optimise(string $png, int $level = 2, bool $interlace = false, bool $optimiseAlpha = false): string
    {
        $returnSlot = $this->wasm->__wbindgen_add_to_stack_pointer(-16);
        try {
            $input = $this->wasm->__wbindgen_malloc(strlen($png), 1);
            $this->wasm->memory->write($input, $png);
            $this->wasm->optimise($returnSlot, $input, strlen($png), $level, (int) $interlace, (int) $optimiseAlpha);

            ['pointer' => $output, 'length' => $length] = unpack('Vpointer/Vlength', $this->wasm->memory->read($returnSlot, 8));
            $optimised = $this->wasm->memory->read($output, $length);
            $this->wasm->__wbindgen_free($output, $length, 1);

            return $optimised;
        } finally {
            $this->wasm->__wbindgen_add_to_stack_pointer(16);
        }
    }
}
