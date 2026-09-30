<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;

require_once __DIR__ . '/RunsPhpInSubprocess.php';
use Wasm\Instance;
use Wasm\Module;

/**
 * wasmtime requires calls into wasm to finish in the order they started, so a
 * PHP callback may not switch fibers while wasm is waiting for it.
 */
final class FiberTest extends TestCase
{
    use RunsPhpInSubprocess;

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

    public function test_a_destructor_run_after_a_callback_cannot_switch_fibers(): void
    {
        $exports = (new Instance(new Module(self::SUSPENDING_IMPORT), [
            // The returned object is released after the callback, when its destructor runs.
            'env' => ['wait' => fn () => new class {
                public function __destruct()
                {
                    \Fiber::suspend();
                }
            }],
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
}
