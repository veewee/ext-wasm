<?php

// Runs a few kinds of wasm through the installed extension: a call, a PHP
// import reading linear memory, a trap, a typed component call and a WASI
// program writing to stdout.

declare(strict_types=1);

function check(string $what, mixed $expected, mixed $actual): void
{
    if ($expected !== $actual) {
        fwrite(STDERR, "$what: expected " . var_export($expected, true) . ', got ' . var_export($actual, true) . "\n");
        exit(1);
    }
}

if (!extension_loaded('wasm')) {
    fwrite(STDERR, "the wasm extension is not loaded\n");
    exit(1);
}

$memory = new Wasm\Memory(['initial' => 1]);
$logged = null;
$module = new Wasm\Module('(module
  (import "env" "log" (func $log (param i32 i32)))
  (import "env" "memory" (memory 1))
  (data (i32.const 0) "hello from wasm")
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1)))
  (func (export "greet") (call $log (i32.const 0) (i32.const 15)))
  (func (export "crash") unreachable))');
$exports = (new Wasm\Instance($module, ['env' => [
    'memory' => $memory,
    'log' => function (int $offset, int $length) use ($memory, &$logged): void {
        $logged = $memory->read($offset, $length);
    },
]]))->exports;

check('add(2, 3)', 5, $exports->add(2, 3));
$exports->greet();
check('the import read guest memory', 'hello from wasm', $logged);
try {
    $exports->crash();
    check('a trap', 'RuntimeError', 'no exception');
} catch (Wasm\Exception\RuntimeError $trap) {
    check('a trap', true, str_contains($trap->getMessage(), 'unreachable'));
}

$component = new Wasm\Component\Component('(component
  (core module $m (func (export "double") (param i32) (result i32) (i32.mul (local.get 0) (i32.const 2))))
  (core instance $i (instantiate $m))
  (func (export "double") (param "n" u32) (result u32) (canon lift (core func $i "double"))))');
check('component double(21)', 42, (new Wasm\Component\Instance($component))->exports->double(21));

$wasi = new Wasm\Wasi();
$program = new Wasm\Module('(module
  (import "wasi_snapshot_preview1" "fd_write" (func $write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "hello from wasi")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 16))
    (i32.store (i32.const 4) (i32.const 15))
    (drop (call $write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))))');
check('WASI exit code', 0, $wasi->start(new Wasm\Instance($program, $wasi->getImportObject())));
check('WASI stdout', 'hello from wasi', $wasi->stdout());

$expected = getenv('VERSION');
if (is_string($expected) && $expected !== '') {
    check('the installed version', $expected, phpversion('wasm'));
}

printf("ext-wasm %s on PHP %s %s %s: ok\n", phpversion('wasm'), PHP_VERSION, PHP_ZTS ? 'zts' : 'nts', php_uname('m'));
