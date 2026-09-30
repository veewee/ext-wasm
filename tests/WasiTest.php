<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Func;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Wasi;

final class WasiTest extends TestCase
{
    public function test_it_captures_stdout_and_returns_exit_code_zero(): void
    {
        $wasi = new Wasi();
        $code = $wasi->start($this->instance($wasi, <<<'EOWAT'
            (data (i32.const 1024) "hi\n")
            (func (export "_start") (call $write (i32.const 1) (i32.const 1024) (i32.const 3)))
            EOWAT));

        self::assertSame(0, $code);
        self::assertSame("hi\n", $wasi->stdout());
        self::assertSame('', $wasi->stderr());
    }

    public function test_it_captures_stderr(): void
    {
        $wasi = new Wasi();
        $wasi->start($this->instance($wasi, <<<'EOWAT'
            (data (i32.const 1024) "oops")
            (func (export "_start") (call $write (i32.const 2) (i32.const 1024) (i32.const 4)))
            EOWAT));

        self::assertSame('oops', $wasi->stderr());
    }

    public function test_proc_exit_becomes_the_exit_code(): void
    {
        $wasi = new Wasi();

        self::assertSame(3, $wasi->start($this->instance($wasi, '(func (export "_start") (call $proc_exit (i32.const 3)))')));
    }

    public function test_the_import_object_holds_funcs_and_combines_with_other_imports(): void
    {
        $wasi = new Wasi();
        $imports = $wasi->getImportObject();

        self::assertInstanceOf(Func::class, $imports['wasi_snapshot_preview1']['fd_write']);

        $module = new Module(self::module(
            '(func (export "_start") (call $proc_exit (call $answer)))',
            '(import "env" "answer" (func $answer (result i32)))',
        ));
        $instance = new Instance($module, [...$imports, 'env' => ['answer' => fn (): int => 9]]);
        self::assertSame(9, $wasi->start($instance));
    }

    private function instance(Wasi $wasi, string $body): Instance
    {
        return new Instance(new Module(self::module($body)), $wasi->getImportObject());
    }

    /** A module with the WASI imports the tests use and $write/$read helpers. */
    private static function module(string $body, string $imports = ''): string
    {
        return <<<EOWAT
            (module
              (import "wasi_snapshot_preview1" "fd_write" (func \$fd_write (param i32 i32 i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_read" (func \$fd_read (param i32 i32 i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "proc_exit" (func \$proc_exit (param i32)))
              (import "wasi_snapshot_preview1" "args_sizes_get" (func \$args_sizes_get (param i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "args_get" (func \$args_get (param i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "environ_sizes_get" (func \$environ_sizes_get (param i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "environ_get" (func \$environ_get (param i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "path_open" (func \$path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
              {$imports}
              (memory (export "memory") 1)
              ;; iovec at 0..8, byte count at 8
              (func \$write (param \$fd i32) (param \$ptr i32) (param \$len i32)
                (i32.store (i32.const 0) (local.get \$ptr))
                (i32.store (i32.const 4) (local.get \$len))
                (drop (call \$fd_write (local.get \$fd) (i32.const 0) (i32.const 1) (i32.const 8))))
              (func \$read (param \$fd i32) (param \$ptr i32) (param \$len i32) (result i32)
                (i32.store (i32.const 0) (local.get \$ptr))
                (i32.store (i32.const 4) (local.get \$len))
                (drop (call \$fd_read (local.get \$fd) (i32.const 0) (i32.const 1) (i32.const 8)))
                (i32.load (i32.const 8)))
              {$body})
            EOWAT;
    }
}
