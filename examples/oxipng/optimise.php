<?php

// Optimises a PNG losslessly with oxipng compiled to wasm.
// Usage: php examples/oxipng/optimise.php [input.png [output.png]]
// Without an input it optimises a generated, deliberately wasteful PNG.

declare(strict_types=1);

require __DIR__ . '/Oxipng.php';

$input = $argv[1] ?? null;
$png = $input === null ? samplePng() : file_get_contents($input);

$start = hrtime(true);
$optimised = (new Oxipng())->optimise($png, level: 2);
$milliseconds = (hrtime(true) - $start) / 1e6;

printf("%s: %d bytes -> %d bytes (%.1f%% smaller) in %.0f ms\n", $input ?? 'sample', strlen($png), strlen($optimised), 100 - 100 * strlen($optimised) / strlen($png), $milliseconds);
if (isset($argv[2])) {
    file_put_contents($argv[2], $optimised);
}

/** A 256x256 RGBA gradient stored without compression, which oxipng shrinks a lot. */
function samplePng(): string
{
    $chunk = static fn (string $type, string $data): string => pack('N', strlen($data)) . $type . $data . pack('N', crc32($type . $data));
    $rows = '';
    for ($y = 0; $y < 256; $y++) {
        $rows .= "\0";
        for ($x = 0; $x < 256; $x++) {
            $rows .= chr($x) . chr($y) . chr(128) . "\xFF";
        }
    }

    return "\x89PNG\r\n\x1a\n"
        . $chunk('IHDR', pack('NNCCCCC', 256, 256, 8, 6, 0, 0, 0))
        . $chunk('IDAT', gzcompress($rows, 0))
        . $chunk('IEND', '');
}
