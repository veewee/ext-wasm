<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\Attributes\RequiresPhpExtension;
use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Instance;
use Wasm\Exception\LinkError;
use Wasm\Exception\RuntimeError;
use Wasm\Wasi;


/**
 * tests/fixtures/component-wasi is a Rust command built for wasm32-wasip2;
 * its first argument picks what it does.
 */
final class ComponentWasiTest extends TestCase
{
    private const FIXTURE = __DIR__ . '/fixtures/component-wasi/component-wasi.wasm';

    private static ?Component $component = null;

    private static function component(): Component
    {
        return self::$component ??= Component::fromFile(self::FIXTURE);
    }

    private static function runCommand(Wasi $wasi): int
    {
        return $wasi->start(new Instance(self::component(), wasi: $wasi));
    }

    private static function directory(string $name): string
    {
        $directory = sys_get_temp_dir() . "/wasm-component-$name-" . bin2hex(random_bytes(6));
        mkdir($directory);

        return $directory;
    }

    public function test_start_runs_the_command_and_returns_its_exit_code(): void
    {
        self::assertSame(0, self::runCommand(new Wasi(args: ['app'])));
        // Rust's std reports a failure through exit(result), which only carries ok or err.
        self::assertSame(1, self::runCommand(new Wasi(args: ['app', 'exit', '3'])));
    }

    public function test_exit_with_code_reports_the_code(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (import "wasi:cli/exit@0.2.12" (instance $exit
                (export "exit-with-code" (func (param "status-code" u8)))))
              (alias export $exit "exit-with-code" (func $exit-with-code))
              (core func $exit-core (canon lower (func $exit-with-code)))
              (core module $m
                (import "wasi" "exit" (func $exit (param i32)))
                (func (export "run") (result i32) (call $exit (i32.const 7)) (i32.const 0)))
              (core instance $i (instantiate $m (with "wasi" (instance (export "exit" (func $exit-core))))))
              (func $run (result (result)) (canon lift (core func $i "run")))
              (instance $run-instance (export "run" (func $run)))
              (export "wasi:cli/run@0.2.0" (instance $run-instance)))
            WAT);
        $wasi = new Wasi();

        self::assertSame(7, $wasi->start(new Instance($component, wasi: $wasi)));
    }

    public function test_args_and_env_reach_the_component(): void
    {
        $wasi = new Wasi(args: ['app', 'print', 'a', 'b'], env: ['GREETING' => 'hi']);

        self::runCommand($wasi);

        self::assertSame("args=a b\ngreeting=hi\n", $wasi->stdout());
    }

    public function test_stdin_reaches_the_component(): void
    {
        $wasi = new Wasi(args: ['app', 'stdin'], stdin: "piped\x00bytes");

        self::runCommand($wasi);

        self::assertSame("piped\x00bytes", $wasi->stdout());
    }

    public function test_a_read_only_preopen_can_be_read_but_not_written(): void
    {
        $in = self::directory('in');
        file_put_contents("$in/data.txt", 'from the host');

        $read = new Wasi(args: ['app', 'read'], preopens: ['/in' => $in]);
        self::runCommand($read);
        self::assertSame('from the host', $read->stdout());

        $write = new Wasi(args: ['app', 'write'], preopens: ['/out' => $in]);
        self::runCommand($write);
        self::assertStringStartsWith('error', $write->stdout());
        self::assertFileDoesNotExist("$in/result.txt");
    }

    public function test_a_writable_preopen_can_be_written(): void
    {
        $out = self::directory('out');
        $wasi = new Wasi(args: ['app', 'write'], preopens: ['/out' => ['path' => $out, 'writable' => true]]);

        self::runCommand($wasi);

        self::assertSame('ok', $wasi->stdout());
        self::assertSame('written', file_get_contents("$out/result.txt"));
    }

    public function test_a_file_stream_read_after_polling_works(): void
    {
        $in = self::directory('stream');
        file_put_contents("$in/data.txt", 'streamed');
        $wasi = new Wasi(args: ['app', 'stream'], preopens: ['/in' => $in]);

        self::runCommand($wasi);

        self::assertSame('streamed', $wasi->stdout());
    }

    public function test_output_past_the_limit_throws_after_the_run(): void
    {
        $wasi = new Wasi(args: ['app', 'flood', '100'], outputLimit: 10);

        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('exceeded the limit of 10 bytes');
        self::runCommand($wasi);
    }

    public function test_a_component_importing_wasi_needs_a_wasi_object(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('wasi:');
        new Instance(self::component());
    }

    public function test_php_entries_cannot_replace_wasi_imports(): void
    {
        $wasi = new Wasi(args: ['app', 'print'], env: ['GREETING' => 'from wasi']);
        $instance = new Instance(self::component(), [
            'wasi:cli/environment' => ['getEnvironment' => fn (): array => [['GREETING', 'from php']]],
        ], $wasi);

        $wasi->start($instance);

        self::assertStringContainsString('greeting=from wasi', $wasi->stdout());
    }

    public function test_a_wasi_object_serves_one_component(): void
    {
        $wasi = new Wasi(args: ['app']);
        new Instance(self::component(), wasi: $wasi);

        $this->expectException(\Error::class);
        $this->expectExceptionMessage('runs one module once');
        new Instance(self::component(), wasi: $wasi);
    }

    public function test_a_wasi_object_used_by_a_core_module_cannot_serve_a_component(): void
    {
        $wasi = new Wasi();
        $wasi->getImportObject();

        $this->expectException(\Error::class);
        $this->expectExceptionMessage('runs one module once');
        new Instance(self::component(), wasi: $wasi);
    }

    public function test_a_wasi_object_used_by_a_component_cannot_serve_a_core_module(): void
    {
        $wasi = new Wasi(args: ['app']);
        new Instance(self::component(), wasi: $wasi);

        $this->expectException(\Error::class);
        $this->expectExceptionMessage('runs one module once');
        $wasi->getImportObject();
    }

    public function test_start_needs_the_instance_the_wasi_object_was_given_to(): void
    {
        $wasi = new Wasi(args: ['app']);
        new Instance(self::component(), wasi: $wasi);
        $other = new Instance(self::component(), wasi: new Wasi(args: ['app']));

        $this->expectException(LinkError::class);
        $wasi->start($other);
    }

    public function test_preview1_modules_still_run(): void
    {
        $wasi = new Wasi(args: ['app']);
        $module = new \Wasm\Module(<<<'WAT'
            (module
              (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 8) "p1\n")
              (func (export "_start")
                (i32.store (i32.const 0) (i32.const 8))
                (i32.store (i32.const 4) (i32.const 3))
                (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 20)))))
            WAT);

        self::assertSame(0, $wasi->start(new \Wasm\Instance($module, $wasi->getImportObject())));
        self::assertSame("p1\n", $wasi->stdout());
    }

    #[RequiresPhpExtension('pcntl')]
    public function test_components_run_in_a_forked_child_after_running_in_the_parent(): void
    {
        $in = self::directory('fork');
        file_put_contents("$in/data.txt", 'forked');
        $run = function (string $mode) use ($in): string {
            $wasi = new Wasi(args: ['app', $mode], preopens: ['/in' => $in]);
            self::runCommand($wasi);

            return $wasi->stdout();
        };
        // The parent's runs are what a prefork server does before forking workers.
        self::assertSame('forked', $run('read'));
        self::assertSame('forked', $run('stream'));

        $pid = pcntl_fork();
        if ($pid === 0) {
            exit($run('read') === 'forked' && $run('stream') === 'forked' ? 0 : 1);
        }

        $deadline = microtime(true) + 20;
        do {
            if (pcntl_waitpid($pid, $status, WNOHANG) === $pid) {
                self::assertSame(0, pcntl_wexitstatus($status));
                return;
            }
            usleep(50_000);
        } while (microtime(true) < $deadline);

        posix_kill($pid, SIGKILL);
        pcntl_waitpid($pid, $status);
        self::fail('The forked child did not finish its WASI file reads within 20 seconds');
    }
}
