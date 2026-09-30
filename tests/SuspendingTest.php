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

    private const LATER_OR_NOW = <<<'EOWAT'
        (module
          (import "env" "later" (func $later (param i32) (result i32)))
          (func (export "run") (param i32) (result i32)
            (i32.add (call $later (local.get 0)) (i32.const 1)))
          (func (export "now") (param i32) (result i32) (local.get 0)))
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

    public function test_an_exception_thrown_after_resume_comes_back_out(): void
    {
        $thrown = new \DomainException('from php');
        $exports = (new Instance(new Module(self::ADD_LATER), [
            'env' => ['later' => new Suspending(function () use ($thrown): int {
                \Fiber::suspend();
                throw $thrown;
            })],
        ]))->exports;

        $fiber = new \Fiber(fn (): int => $exports->run(1));
        $fiber->start();

        try {
            $fiber->resume();
            self::fail('Expected the callback exception to propagate');
        } catch (\DomainException $caught) {
            self::assertSame($thrown, $caught);
        }
    }

    public function test_wasm_catches_a_wasm_throw_thrown_after_resume(): void
    {
        $tag = new \Wasm\Tag(['parameters' => ['i32']]);
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "e" (tag $e (param i32)))
              (import "env" "fail" (func $fail))
              (func (export "run") (result i32)
                (block $caught (result i32)
                  (try_table (catch $e $caught) (call $fail))
                  (i32.const -1))
                (i32.const 1)
                (i32.add)))
            EOWAT), ['env' => ['e' => $tag, 'fail' => new Suspending(function () use ($tag): void {
            \Fiber::suspend();
            throw new \Wasm\Exception\WasmThrow($tag, [41]);
        })]]))->exports;

        $fiber = new \Fiber(fn (): int => $exports->run());
        $fiber->start();
        $fiber->resume();

        self::assertSame(42, $fiber->getReturn());
    }

    public function test_suspending_outside_a_fiber_is_a_fiber_error(): void
    {
        $exports = (new Instance(new Module(self::ADD_LATER), [
            'env' => ['later' => new Suspending(fn (): int => \Fiber::suspend())],
        ]))->exports;

        $this->expectException(\FiberError::class);
        $exports->run(1);
    }

    public function test_the_instance_stays_usable_after_a_failed_suspending_call(): void
    {
        $fail = true;
        $exports = (new Instance(new Module(self::ADD_LATER), [
            'env' => ['later' => new Suspending(function (int $n) use (&$fail): int {
                if ($fail) {
                    throw new \DomainException('once');
                }

                return $n;
            })],
        ]))->exports;

        try {
            $exports->run(1);
            self::fail('Expected the callback exception to propagate');
        } catch (\DomainException) {
        }
        $fail = false;

        self::assertSame(3, $exports->run(2));
    }

    public function test_a_destroyed_suspended_fiber_unwinds_and_leaves_the_instance_usable(): void
    {
        $script = <<<'PHP'
            <?php
            $exports = (new Wasm\Instance(new Wasm\Module('%s'), [
                'env' => ['later' => new Wasm\Suspending(function (int $n): int {
                    try {
                        return Fiber::suspend();
                    } finally {
                        echo "unwound\n";
                    }
                })],
            ]))->exports;
            $fiber = new Fiber(fn (): int => $exports->run(1));
            $fiber->start();
            unset($fiber);
            echo $exports->now(1) === 1 ? "usable\n" : "broken\n";
            PHP;

        $output = $this->runPhp(sprintf($script, self::LATER_OR_NOW), $exitCode);

        self::assertSame(0, $exitCode, $output);
        self::assertSame("unwound\nusable", $output);
    }

    public function test_exit_inside_a_suspended_callback_exits_the_process(): void
    {
        $script = <<<'PHP'
            <?php
            $exports = (new Wasm\Instance(new Wasm\Module('%s'), [
                'env' => ['later' => new Wasm\Suspending(function (): int {
                    Fiber::suspend();
                    echo "exiting\n";
                    exit(3);
                })],
            ]))->exports;
            $fiber = new Fiber(fn (): int => $exports->run(1));
            $fiber->start();
            $fiber->resume();
            echo "not reached\n";
            PHP;

        $output = $this->runPhp(sprintf($script, self::ADD_LATER), $exitCode);

        self::assertSame(3, $exitCode, $output);
        self::assertSame('exiting', $output);
    }

    public function test_a_fiber_left_suspended_at_script_end_does_not_crash(): void
    {
        $script = <<<'PHP'
            <?php
            $exports = (new Wasm\Instance(new Wasm\Module('%s'), [
                'env' => ['later' => new Wasm\Suspending(fn (): int => Fiber::suspend())],
            ]))->exports;
            $fiber = new Fiber(fn (): int => $exports->run(1));
            $fiber->start();
            echo "done";
            PHP;

        $output = $this->runPhp(sprintf($script, self::ADD_LATER), $exitCode);

        self::assertSame(0, $exitCode, $output);
        self::assertSame('done', $output);
    }
}
