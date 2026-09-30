<?php

declare(strict_types=1);

/**
 * Formats a PHP file with mago's formatter, running mago's official wasm build.
 *
 *     examples/mago/download.sh
 *     php examples/mago/format.php path/to/file.php [php-version]
 */

[$script, $file] = $argv + [null, null];
if ($file === null) {
    fwrite(STDERR, "usage: php $script <file.php> [php-version]\n");
    exit(1);
}
$phpVersion = $argv[2] ?? PHP_MAJOR_VERSION . '.' . PHP_MINOR_VERSION;

$wasm = __DIR__ . '/dist/mago_wasm_bg.wasm';
if (!is_file($wasm)) {
    fwrite(STDERR, "The mago wasm build is missing, run examples/mago/download.sh first.\n");
    exit(1);
}

$module = new Wasm\Module(file_get_contents($wasm));

// The build is made for JavaScript and declares JS helper functions as imports.
// Formatting never calls them, so every import is a stub that fails loudly.
$imports = [];
foreach (Wasm\Module::imports($module) as $import) {
    $imports[$import['module']][$import['name']] = static function () use ($import): never {
        throw new RuntimeException("mago needed the JavaScript helper {$import['name']}, which usually means the input could not be formatted");
    };
}
$mago = (new Wasm\Instance($module, $imports))->exports;

try {
    echo format($mago, file_get_contents($file), $phpVersion);
} catch (RuntimeException $error) {
    fwrite(STDERR, $error->getMessage() . "\n");
    exit(1);
}

/**
 * Calls `format(code, version)` the way the generated JS glue does: strings go
 * in as pointer and length in wasm memory, and the result comes back through a
 * 16 byte area on wasm's stack holding pointer, length and an error flag.
 */
function format(Wasm\Exports $mago, string $code, string $phpVersion): string
{
    $result = $mago->__wbindgen_add_to_stack_pointer(-16);
    try {
        $mago->format($result, ...toWasmString($mago, $code), ...toWasmString($mago, $phpVersion));
        [$pointer, $length, , $failed] = array_values(unpack('V4', $mago->memory->read($result, 16)));
        if ($failed) {
            throw new RuntimeException('mago could not format the input');
        }

        return $mago->memory->read($pointer, $length);
    } finally {
        $mago->__wbindgen_add_to_stack_pointer(16);
    }
}

/** @return array{int, int} pointer and length of a copy of $value in wasm memory */
function toWasmString(Wasm\Exports $mago, string $value): array
{
    // __wbindgen_export is the module's malloc(size, align).
    $pointer = $mago->__wbindgen_export(strlen($value), 1) & 0xFFFFFFFF;
    $mago->memory->write($pointer, $value);

    return [$pointer, strlen($value)];
}
