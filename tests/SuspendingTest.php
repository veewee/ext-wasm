<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\LinkError;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Suspending;

require_once __DIR__ . '/RunsPhpInSubprocess.php';

/**
 * A Suspending import may pause its Fiber while wasm waits for it, like
 * WebAssembly.Suspending with JS Promise Integration.
 */
final class SuspendingTest extends TestCase
{
    use RunsPhpInSubprocess;

    private const ADD_LATER = <<<'EOWAT'
        (module
          (import "env" "later" (func $later (param i32) (result i32)))
          (func (export "run") (param i32) (result i32)
            (i32.add (call $later (local.get 0)) (i32.const 1))))
        EOWAT;

    public function test_a_suspending_import_suspends_and_resumes_its_fiber(): void
    {
        $exports = (new Instance(new Module(self::ADD_LATER), [
            'env' => ['later' => new Suspending(fn (int $n): int => \Fiber::suspend($n) * 10)],
        ]))->exports;

        $fiber = new \Fiber(fn (): int => $exports->run(4));

        self::assertSame(4, $fiber->start());
        $fiber->resume(5);
        self::assertTrue($fiber->isTerminated());
        self::assertSame(51, $fiber->getReturn());
    }

    public function test_a_suspending_callback_may_return_without_suspending(): void
    {
        $exports = (new Instance(new Module(self::ADD_LATER), [
            'env' => ['later' => new Suspending(fn (int $n): int => $n * 2)],
        ]))->exports;

        self::assertSame(7, $exports->run(3));
    }

    public function test_the_constructor_requires_a_callable(): void
    {
        $this->expectException(\TypeError::class);
        new Suspending('not a function name');
    }

    public function test_suspending_for_a_non_function_import_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        new Instance(new Module('(module (import "env" "g" (global i32)))'), [
            'env' => ['g' => new Suspending(fn () => 1)],
        ]);
    }

    public function test_a_suspending_callback_reads_and_writes_memory_while_suspended(): void
    {
        $instance = null;
        $instance = new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "fill" (func $fill (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "run") (result i32) (call $fill (i32.const 16) (i32.const 8))))
            EOWAT), ['env' => ['fill' => new Suspending(function (int $ptr, int $cap) use (&$instance): int {
            $text = \Fiber::suspend();
            $instance->exports->memory->write($ptr, substr($text, 0, $cap));

            return min(strlen($text), $cap);
        })]]);

        $fiber = new \Fiber(fn (): int => $instance->exports->run());
        $fiber->start();
        $fiber->resume('hello');

        self::assertSame(5, $fiber->getReturn());
        self::assertSame('hello', $instance->exports->memory->read(16, 5));
    }

    public function test_plain_callbacks_in_an_async_store_still_block_fiber_switches(): void
    {
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "later" (func $later))
              (import "env" "plain" (func $plain))
              (func (export "run") (call $plain) (call $later)))
            EOWAT), ['env' => [
            'later' => new Suspending(fn () => null),
            'plain' => fn () => \Fiber::suspend(),
        ]]))->exports;

        $fiber = new \Fiber(fn () => $exports->run());

        $this->expectException(\FiberError::class);
        $fiber->start();
    }

    public function test_a_plain_import_listed_before_a_suspending_one_works(): void
    {
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "plain" (func $plain (result i32)))
              (import "env" "later" (func $later (param i32) (result i32)))
              (func (export "run") (result i32) (call $later (call $plain))))
            EOWAT), ['env' => [
            'plain' => fn (): int => 20,
            'later' => new Suspending(fn (int $n): int => \Fiber::suspend() + $n),
        ]]))->exports;

        $fiber = new \Fiber(fn (): int => $exports->run());
        $fiber->start();
        $fiber->resume(22);

        self::assertSame(42, $fiber->getReturn());
    }

    public function test_plain_callbacks_in_an_async_store_run_on_the_php_stack(): void
    {
        $depth = function (int $n) use (&$depth): int {
            return $n === 0 ? 0 : 1 + $depth($n - 1);
        };
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "later" (func $later))
              (import "env" "deep" (func $deep (result i32)))
              (func (export "run") (result i32) (call $later) (call $deep)))
            EOWAT), ['env' => [
            'later' => new Suspending(fn () => null),
            'deep' => fn (): int => $depth(20000),
        ]]))->exports;

        self::assertSame(20000, $exports->run());
    }

    public function test_the_start_function_may_call_a_suspending_import(): void
    {
        $module = new Module(<<<'EOWAT'
            (module
              (import "env" "later" (func $later (result i32)))
              (global $seen (export "seen") (mut i32) (i32.const 0))
              (func $init (global.set $seen (call $later)))
              (start $init))
            EOWAT);

        $fiber = new \Fiber(fn (): Instance => new Instance($module, [
            'env' => ['later' => new Suspending(fn (): int => \Fiber::suspend())],
        ]));
        $fiber->start();
        $fiber->resume(9);

        self::assertSame(9, $fiber->getReturn()->exports->seen->value);
    }

    public function test_destructors_of_values_released_by_a_suspending_callback_can_use_wasm(): void
    {
        $memory = new \Wasm\Memory(['initial' => 1]);
        $probe = new \ArrayObject();
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "memory" (memory 1))
              (import "env" "f" (func $f))
              (func (export "run") (call $f)))
            EOWAT), ['env' => ['memory' => $memory, 'f' => new Suspending(function () use ($memory, $probe): object {
            \Fiber::suspend();

            // Discarded because the import has no results.
            return new class ($memory, $probe) {
                public function __construct(private \Wasm\Memory $memory, private \ArrayObject $probe)
                {
                }

                public function __destruct()
                {
                    $this->probe['seen'] = $this->memory->byteLength();
                }
            };
        })]]))->exports;

        $fiber = new \Fiber(fn () => $exports->run());
        $fiber->start();
        $fiber->resume();

        self::assertSame(65536, $probe['seen'] ?? null);
    }

    public function test_fibers_resumed_out_of_order_do_not_crash(): void
    {
        $script = <<<'PHP'
            <?php
            $module = new Wasm\Module('%s');
            $fibers = [];
            foreach (['a', 'b', 'c'] as $name) {
                $exports = (new Wasm\Instance($module, [
                    'env' => ['later' => new Wasm\Suspending(fn (int $n): int => Fiber::suspend() + $n)],
                ]))->exports;
                $fibers[$name] = new Fiber(fn (): int => $exports->run(1));
                $fibers[$name]->start();
            }
            $fibers['b']->resume(10);
            $fibers['c']->resume(20);
            $fibers['a']->resume(30);
            echo implode(',', array_map(fn (Fiber $f): int => $f->getReturn(), $fibers));
            PHP;

        $output = $this->runPhp(sprintf($script, self::ADD_LATER), $exitCode);

        self::assertSame(0, $exitCode, $output);
        self::assertSame('32,12,22', $output);
    }
}
