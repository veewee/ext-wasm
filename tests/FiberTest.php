<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Instance;
use Wasm\Module;

/**
 * wasmtime requires calls into wasm to finish in the order they started, so a
 * PHP callback may not switch fibers while wasm is waiting for it.
 */
final class FiberTest extends TestCase
{
    private const SUSPENDING_IMPORT = '(module (import "env" "wait" (func $wait)) (func (export "run") (call $wait)))';

    public function test_suspending_a_fiber_inside_a_callback_throws_a_fiber_error(): void
    {
        $exports = (new Instance(new Module(self::SUSPENDING_IMPORT), [
            'env' => ['wait' => fn () => \Fiber::suspend()],
        ]))->exports;

        $fiber = new \Fiber(fn () => $exports->run());

        $this->expectException(\FiberError::class);
        $fiber->start();
    }

    public function test_interleaved_fibers_cannot_crash_the_process(): void
    {
        $script = <<<'PHP'
            <?php
            $exports = (new Wasm\Instance(new Wasm\Module('%s'), [
                'env' => ['wait' => fn () => Fiber::suspend()],
            ]))->exports;
            $outcomes = [];
            $a = new Fiber(function () use ($exports, &$outcomes) {
                try { $exports->run(); $outcomes[] = 'a ran'; } catch (FiberError) { $outcomes[] = 'a blocked'; }
            });
            $b = new Fiber(function () use ($exports, &$outcomes) {
                try { $exports->run(); $outcomes[] = 'b ran'; } catch (FiberError) { $outcomes[] = 'b blocked'; }
            });
            $a->start();
            $b->start();
            if ($a->isSuspended()) { $a->resume(); }
            if ($b->isSuspended()) { $b->resume(); }
            echo implode(',', $outcomes);
            PHP;

        $output = $this->runPhp(sprintf($script, self::SUSPENDING_IMPORT), $exitCode);

        self::assertSame(0, $exitCode, $output);
        self::assertSame('a blocked,b blocked', $output);
    }

    public function test_wasm_runs_inside_fibers_that_suspend_between_calls(): void
    {
        $exports = (new Instance(new Module('(module (func (export "id") (param i32) (result i32) local.get 0))')))->exports;
        $fiber = new \Fiber(function () use ($exports): int {
            $first = $exports->id(1);
            \Fiber::suspend();

            return $first + $exports->id(2);
        });

        $fiber->start();
        self::assertSame(5, $exports->id(5));
        $fiber->resume();

        self::assertSame(3, $fiber->getReturn());
    }

    private function runPhp(string $code, ?int &$exitCode): string
    {
        $file = tempnam(sys_get_temp_dir(), 'wasm-fiber');
        file_put_contents($file, $code);
        $command = [PHP_BINARY, '-n', '-d', 'extension=' . self::loadedExtensionPath(), $file];
        $process = proc_open($command, [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
        $output = stream_get_contents($pipes[1]) . stream_get_contents($pipes[2]);
        $exitCode = proc_close($process);
        unlink($file);

        return trim($output);
    }

    /** The built extension this suite runs against, for child processes. */
    private static function loadedExtensionPath(): string
    {
        $override = getenv('WASM_EXTENSION');
        if (is_string($override) && $override !== '') {
            return $override;
        }
        $candidates = glob(dirname(__DIR__) . '/target/{release,debug}/{libwasm.so,libwasm.dylib,wasm.dll}', GLOB_BRACE) ?: [];
        usort($candidates, fn (string $a, string $b): int => filemtime($b) <=> filemtime($a));

        return $candidates[0] ?? 'wasm';
    }
}
