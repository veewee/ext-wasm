<?php

declare(strict_types=1);

/**
 * Runs mago's formatter or static analyzer from PHP through its wasm build.
 *
 *     php examples/mago/mago.php format   path/to/file.php
 *     php examples/mago/mago.php analyze  path/to/file.php
 */

require __DIR__ . '/WasmBindgenHost.php';

use Example\Mago\WasmBindgenHost;

[$script, $command, $file] = $argv + [null, null, null];
if (!in_array($command, ['format', 'analyze'], true) || $file === null) {
    fwrite(STDERR, "usage: php $script format|analyze <file.php> [php-version]\n");
    exit(1);
}
$phpVersion = $argv[3] ?? PHP_MAJOR_VERSION . '.' . PHP_MINOR_VERSION;

$dist = __DIR__ . '/dist';
if (!is_file("$dist/mago_wasm_bg.wasm")) {
    fwrite(STDERR, "The mago wasm build is missing, run examples/mago/download.sh first.\n");
    exit(1);
}

$started = microtime(true);
$mago = new WasmBindgenHost(
    new Wasm\Module(file_get_contents("$dist/mago_wasm_bg.wasm")),
    file_get_contents("$dist/mago_wasm_bg.js"),
);
$loaded = microtime(true);

$code = file_get_contents($file);
if ($command === 'format') {
    echo $mago->callReturningString('format', $code, $phpVersion);
} else {
    $issues = $mago->callReturningValue('analyze', $code, $phpVersion);
    foreach ($issues as $issue) {
        printf("%s [%s] %s\n", strtoupper($issue['level'] ?? 'issue'), $issue['code'] ?? '?', $issue['message'] ?? json_encode($issue));
    }
    printf("%d issue(s)\n", count($issues));
}

fprintf(STDERR, "loaded mago in %.2fs, ran %s in %.2fs\n", $loaded - $started, $command, microtime(true) - $loaded);
