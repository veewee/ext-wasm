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
    /** Opens hello.txt in the first preopen and copies it to stdout; exits with the errno on failure. */
    public const OPEN_AND_READ = <<<'EOWAT'
        (data (i32.const 512) "hello.txt")
        (func (export "_start") (local $err i32)
          (local.set $err (call $path_open (i32.const 3) (i32.const 0) (i32.const 512) (i32.const 9)
            (i32.const 0) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 12)))
          (if (local.get $err) (then (call $proc_exit (local.get $err))))
          (call $write (i32.const 1) (i32.const 1024) (call $read (i32.load (i32.const 12)) (i32.const 1024) (i32.const 1024))))
        EOWAT;

    private const CREATE_AND_WRITE = <<<'EOWAT'
        (data (i32.const 512) "out.txt")
        (data (i32.const 1024) "made")
        (func (export "_start") (local $err i32)
          (local.set $err (call $path_open (i32.const 3) (i32.const 0) (i32.const 512) (i32.const 7)
            (i32.const 9) (i64.const 64) (i64.const 0) (i32.const 0) (i32.const 12)))
          (if (local.get $err) (then (call $proc_exit (local.get $err))))
          (call $write (i32.load (i32.const 12)) (i32.const 1024) (i32.const 4)))
        EOWAT;

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

    public function test_args_reach_the_module(): void
    {
        $wasi = new Wasi(args: ['prog', '--flag']);
        $wasi->start($this->instance($wasi, <<<'EOWAT'
            (func (export "_start")
              (drop (call $args_sizes_get (i32.const 16) (i32.const 20)))
              (drop (call $args_get (i32.const 2048) (i32.const 1024)))
              (call $write (i32.const 1) (i32.const 1024) (i32.load (i32.const 20))))
            EOWAT));

        self::assertSame("prog\0--flag\0", $wasi->stdout());
    }

    public function test_env_is_empty_unless_given(): void
    {
        foreach ([[null, ''], [['LANG' => 'C'], "LANG=C\0"]] as [$env, $expected]) {
            $wasi = new Wasi(env: $env);
            $wasi->start($this->instance($wasi, <<<'EOWAT'
                (func (export "_start")
                  (drop (call $environ_sizes_get (i32.const 16) (i32.const 20)))
                  (drop (call $environ_get (i32.const 2048) (i32.const 1024)))
                  (call $write (i32.const 1) (i32.const 1024) (i32.load (i32.const 20))))
                EOWAT));

            self::assertSame($expected, $wasi->stdout());
        }
    }

    public function test_stdin_is_the_given_string(): void
    {
        $wasi = new Wasi(stdin: 'hello');
        $wasi->start($this->instance($wasi, <<<'EOWAT'
            (func (export "_start")
              (call $write (i32.const 1) (i32.const 1024) (call $read (i32.const 0) (i32.const 1024) (i32.const 1024))))
            EOWAT));

        self::assertSame('hello', $wasi->stdout());
    }

    public function test_output_up_to_the_limit_is_kept(): void
    {
        $wasi = new Wasi(outputLimit: 1024);

        self::assertSame(0, $wasi->start($this->instance($wasi, '(func (export "_start") (call $write (i32.const 1) (i32.const 1024) (i32.const 1024)))')));
        self::assertSame(1024, strlen($wasi->stdout()));
    }

    /** @return iterable<string, array{int}> */
    public static function outputStreams(): iterable
    {
        yield 'stdout' => [1];
        yield 'stderr' => [2];
    }

    #[\PHPUnit\Framework\Attributes\DataProvider('outputStreams')]
    public function test_output_past_the_limit_is_a_runtime_error(int $fd): void
    {
        $wasi = new Wasi(outputLimit: 100);
        $instance = $this->instance($wasi, "(func (export \"_start\") (call \$write (i32.const {$fd}) (i32.const 1024) (i32.const 1024)))");

        try {
            $wasi->start($instance);
            self::fail('Expected a RuntimeError');
        } catch (\Wasm\Exception\RuntimeError $error) {
            self::assertStringContainsString('exceeded the limit of 100 bytes', $error->getMessage());
        }
        self::assertSame(100, strlen($fd === 1 ? $wasi->stdout() : $wasi->stderr()));
    }

    public function test_a_preopened_directory_can_be_read(): void
    {
        $dir = self::tempDir();
        file_put_contents("$dir/hello.txt", 'from the host');
        $wasi = new Wasi(preopens: ['/data' => $dir]);

        self::assertSame(0, $wasi->start($this->instance($wasi, self::OPEN_AND_READ)));
        self::assertSame('from the host', $wasi->stdout());
    }

    public function test_preopens_are_read_only_by_default(): void
    {
        $dir = self::tempDir();
        $wasi = new Wasi(preopens: ['/data' => $dir]);

        self::assertNotSame(0, $wasi->start($this->instance($wasi, self::CREATE_AND_WRITE)));
        self::assertFileDoesNotExist("$dir/out.txt");
    }

    public function test_a_writable_preopen_can_be_written(): void
    {
        $dir = self::tempDir();
        $wasi = new Wasi(preopens: ['/data' => ['path' => $dir, 'writable' => true]]);

        self::assertSame(0, $wasi->start($this->instance($wasi, self::CREATE_AND_WRITE)));
        self::assertSame('made', file_get_contents("$dir/out.txt"));
    }

    public function test_a_missing_preopen_directory_throws(): void
    {
        $this->expectException(\ValueError::class);
        new Wasi(preopens: ['/data' => sys_get_temp_dir() . '/does-not-exist-' . uniqid()]);
    }

    public static function tempDir(): string
    {
        $dir = sys_get_temp_dir() . '/wasm-wasi-' . uniqid();
        mkdir($dir);

        return $dir;
    }

    private function instance(Wasi $wasi, string $body): Instance
    {
        return new Instance(new Module(self::module($body)), $wasi->getImportObject());
    }

    /** A module with the WASI imports the tests use and $write/$read helpers. */
    public static function module(string $body, string $imports = ''): string
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
