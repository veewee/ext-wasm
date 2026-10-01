<?php

declare(strict_types=1);

use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Component\Stream;

/**
 * Streaming gzip by the async Rust component in src/lib.rs.
 *
 * wit/gzip.wit declares compress(input: stream<u8>, level: u32) -> stream<u8>:
 * PHP passes any iterable of byte strings, and gets a Stream of compressed
 * chunks back. The component asks for input only as it needs it, and writes
 * output only while PHP reads, so memory stays flat whatever the size.
 */
final class Gzip
{
    private readonly Exports $gzip;

    public function __construct(string $wasmFile = __DIR__ . '/stream-gzip.wasm')
    {
        $instance = new Instance(Component::fromFile($wasmFile), wasi: new Wasm\Wasi());
        $this->gzip = $instance->exports->get('docs:stream-gzip/gzip');
    }

    /** @param iterable<string> $chunks */
    public function compress(iterable $chunks, int $level = 6): Stream
    {
        return $this->gzip->compress($chunks, $level);
    }

    /** @return \Generator<string> the file in chunks of $size bytes */
    public static function read(string $file, int $size = 64 * 1024): \Generator
    {
        $handle = fopen($file, 'rb') ?: throw new RuntimeException("cannot open $file");
        try {
            while (!feof($handle)) {
                $chunk = fread($handle, $size);
                if ($chunk !== false && $chunk !== '') {
                    yield $chunk;
                }
            }
        } finally {
            fclose($handle);
        }
    }
}
