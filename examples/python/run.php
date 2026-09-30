<?php

// Runs Python code given on the command line in CPython 3.12 compiled to WASI.
// Usage: php examples/python/run.php 'print(1 + 1)' < optional-stdin

declare(strict_types=1);

$code = $argv[1] ?? 'print("Hello from Python in PHP")';
$module = Wasm\Module::fromFile(__DIR__ . '/dist/python.wasm');
$stdin = stream_isatty(STDIN) ? '' : stream_get_contents(STDIN);

$wasi = new Wasm\Wasi(args: ['python', '-c', $code], stdin: $stdin);
$exitCode = $wasi->start(new Wasm\Instance($module, $wasi->getImportObject()));

fwrite(STDOUT, $wasi->stdout());
fwrite(STDERR, $wasi->stderr());
exit($exitCode);
