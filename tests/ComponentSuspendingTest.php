<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Exception\RuntimeError;
use Wasm\Suspending;

require_once __DIR__ . '/RunsPhpInSubprocess.php';

/**
 * Wasm\Suspending as a component import: the callable may suspend its Fiber
 * while the component waits for it.
 */
final class ComponentSuspendingTest extends TestCase
{
    use RunsPhpInSubprocess;

    /** Imports later(n) and plain(), and exports run(n) = later(n) + 1, both() and id(n). */
    private const COMPONENT = <<<'WAT'
        (component
          (import "later" (func $later (param "n" u32) (result u32)))
          (import "plain" (func $plain (result u32)))
          (core func $later-core (canon lower (func $later)))
          (core func $plain-core (canon lower (func $plain)))
          (core module $m
            (import "host" "later" (func $later (param i32) (result i32)))
            (import "host" "plain" (func $plain (result i32)))
            (func (export "run") (param i32) (result i32) (i32.add (call $later (local.get 0)) (i32.const 1)))
            (func (export "both") (result i32) (i32.add (call $plain) (call $later (i32.const 0))))
            (func (export "id") (param i32) (result i32) (local.get 0)))
          (core instance $i (instantiate $m (with "host" (instance
            (export "later" (func $later-core))
            (export "plain" (func $plain-core))))))
          (func (export "run") (param "n" u32) (result u32) (canon lift (core func $i "run")))
          (func (export "both") (result u32) (canon lift (core func $i "both")))
          (func (export "id") (param "n" u32) (result u32) (canon lift (core func $i "id"))))
        WAT;

    /** Exports a resource thing whose constructor awaits later(); value() gives what it returned. */
    private const RESOURCES = <<<'WAT'
        (component
          (import "later" (func $later (result u32)))
          (core func $later-core (canon lower (func $later)))
          (type $thing' (resource (rep i32)))
          (core func $new (canon resource.new $thing'))
          (core module $m
            (import "host" "new" (func $new (param i32) (result i32)))
            (import "host" "later" (func $later (result i32)))
            (func (export "ctor") (result i32) (call $new (call $later)))
            ;; A borrow of a resource the component defines arrives as its representation.
            (func (export "value") (param i32) (result i32) (local.get 0)))
          (core instance $i (instantiate $m (with "host" (instance
            (export "new" (func $new))
            (export "later" (func $later-core))))))
          (export $thing "thing" (type $thing'))
          (func (export "[constructor]thing") (result (own $thing)) (canon lift (core func $i "ctor")))
          (func (export "[method]thing.value") (param "self" (borrow $thing)) (result u32) (canon lift (core func $i "value"))))
        WAT;

    /** @param array<string, mixed> $imports */
    private static function exports(array $imports): Exports
    {
        return (new Instance(new Component(self::COMPONENT), $imports + ['plain' => fn (): int => 10]))->exports;
    }

    public function test_a_suspending_import_suspends_and_resumes_its_fiber(): void
    {
        $exports = self::exports(['later' => new Suspending(fn (int $n): int => \Fiber::suspend($n) * 10)]);
        $fiber = new \Fiber(fn (): int => $exports->run(4));

        self::assertSame(4, $fiber->start());
        $fiber->resume(5);

        self::assertSame(51, $fiber->getReturn());
    }

    public function test_a_suspending_import_may_return_without_suspending(): void
    {
        self::assertSame(7, self::exports(['later' => new Suspending(fn (int $n): int => $n * 2)])->run(3));
    }

    public function test_a_plain_import_of_an_async_instance_still_blocks_fiber_switches(): void
    {
        $exports = self::exports([
            'later' => new Suspending(fn (int $n): int => $n),
            'plain' => fn (): int => \Fiber::suspend(),
        ]);

        $this->expectException(\FiberError::class);
        (new \Fiber(fn (): int => $exports->both()))->start();
    }

    public function test_another_fiber_cannot_call_into_a_parked_instance(): void
    {
        $exports = self::exports(['later' => new Suspending(fn (int $n): int => \Fiber::suspend())]);
        $fiber = new \Fiber(fn (): int => $exports->run(1));
        $fiber->start();

        try {
            $exports->id(1);
            self::fail('Expected the store to be busy');
        } catch (RuntimeError $busy) {
            self::assertSame('the store is busy with a suspended call', $busy->getMessage());
        }
        $fiber->resume(2);
        self::assertSame(3, $fiber->getReturn());
        self::assertSame(5, $exports->id(5));
    }

    public function test_an_exception_thrown_after_resume_comes_back_out(): void
    {
        $thrown = new \DomainException('after resume');
        $exports = self::exports(['later' => new Suspending(function () use ($thrown): int {
            \Fiber::suspend();
            throw $thrown;
        })]);
        $fiber = new \Fiber(fn (): int => $exports->run(1));
        $fiber->start();

        try {
            $fiber->resume();
            self::fail('Expected the import exception');
        } catch (\DomainException $caught) {
            self::assertSame($thrown, $caught);
        }

        // A failed component call leaves its instance trapped.
        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('cannot enter component instance');
        $exports->id(1);
    }

    public function test_fibers_resumed_out_of_order_do_not_crash(): void
    {
        $script = <<<'PHP'
            <?php
            $component = new Wasm\Component\Component('%s');
            $fibers = [];
            foreach (['a', 'b', 'c'] as $name) {
                $exports = (new Wasm\Component\Instance($component, [
                    'later' => new Wasm\Suspending(fn (int $n): int => Fiber::suspend() + $n),
                    'plain' => fn (): int => 0,
                ]))->exports;
                $fibers[$name] = new Fiber(fn (): int => $exports->run(1));
                $fibers[$name]->start();
            }
            $fibers['b']->resume(10);
            $fibers['c']->resume(20);
            $fibers['a']->resume(30);
            echo implode(',', array_map(fn (Fiber $f): int => $f->getReturn(), $fibers));
            PHP;

        $output = $this->runPhp(sprintf($script, self::COMPONENT), $exitCode);

        self::assertSame(0, $exitCode, $output);
        self::assertSame('32,12,22', $output);
    }

    public function test_a_destroyed_suspended_fiber_unwinds(): void
    {
        $script = <<<'PHP'
            <?php
            $exports = (new Wasm\Component\Instance(new Wasm\Component\Component('%s'), [
                'later' => new Wasm\Suspending(function (int $n): int {
                    try {
                        return Fiber::suspend();
                    } finally {
                        echo "unwound\n";
                    }
                }),
                'plain' => fn (): int => 0,
            ]))->exports;
            $fiber = new Fiber(fn (): int => $exports->run(1));
            $fiber->start();
            unset($fiber);
            echo "done\n";
            PHP;

        $output = $this->runPhp(sprintf($script, self::COMPONENT), $exitCode);

        self::assertSame(0, $exitCode, $output);
        self::assertSame("unwound\ndone", $output);
    }

    public function test_wasi_functions_work_in_an_async_instance(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (import "wasi:clocks/monotonic-clock@0.2.0" (instance $clock
                (export "now" (func (result u64)))))
              (alias export $clock "now" (func $now))
              (import "later" (func $later (param "n" u32) (result u32)))
              (core func $now-core (canon lower (func $now)))
              (core func $later-core (canon lower (func $later)))
              (core module $m
                (import "host" "now" (func $now (result i64)))
                (import "host" "later" (func $later (param i32) (result i32)))
                (func (export "run") (result i32)
                  (i32.add (call $later (i32.const 1)) (i64.gt_u (call $now) (i64.const 0)))))
              (core instance $i (instantiate $m (with "host" (instance
                (export "now" (func $now-core))
                (export "later" (func $later-core))))))
              (func (export "run") (result u32) (canon lift (core func $i "run"))))
            WAT);
        $exports = (new Instance($component, [
            'later' => new Suspending(fn (int $n): int => \Fiber::suspend() + $n),
        ], new \Wasm\Wasi()))->exports;

        $fiber = new \Fiber(fn (): int => $exports->run());
        $fiber->start();
        $fiber->resume(40);

        self::assertSame(42, $fiber->getReturn());
    }

    public function test_wasi_start_runs_a_command_with_a_suspending_import(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (import "later" (func $later (result u32)))
              (core func $later-core (canon lower (func $later)))
              (core module $m
                (import "host" "later" (func $later (result i32)))
                ;; run() -> result: ok when later() returned 7
                (func (export "run") (result i32) (i32.ne (call $later) (i32.const 7))))
              (core instance $i (instantiate $m (with "host" (instance (export "later" (func $later-core))))))
              (func $run (result (result)) (canon lift (core func $i "run")))
              (instance $run-instance (export "run" (func $run)))
              (export "wasi:cli/run@0.2.0" (instance $run-instance)))
            WAT);
        $wasi = new \Wasm\Wasi();
        $instance = new Instance($component, ['later' => new Suspending(fn (): int => \Fiber::suspend())], $wasi);

        $fiber = new \Fiber(fn (): int => $wasi->start($instance));
        $fiber->start();
        $fiber->resume(7);

        self::assertSame(0, $fiber->getReturn());
    }

    public function test_resources_work_in_an_async_instance(): void
    {
        $component = new Component(self::RESOURCES);
        $exports = (new Instance($component, ['later' => new Suspending(fn (): int => \Fiber::suspend())]))->exports;

        $fiber = new \Fiber(fn () => $exports->get('thing')->new());
        $fiber->start();
        $fiber->resume(9);
        $thing = $fiber->getReturn();

        self::assertSame(9, $thing->value());
        $thing->drop();
        $this->expectException(\Error::class);
        $thing->value();
    }

    public function test_a_suspending_function_of_an_imported_interface_suspends(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (import "docs:demo/slow@0.1.0" (instance $slow
                (export "later" (func (param "n" u32) (result u32)))))
              (alias export $slow "later" (func $later))
              (core func $later-core (canon lower (func $later)))
              (core module $m
                (import "host" "later" (func $later (param i32) (result i32)))
                (func (export "run") (param i32) (result i32) (call $later (local.get 0))))
              (core instance $i (instantiate $m (with "host" (instance (export "later" (func $later-core))))))
              (func (export "run") (param "n" u32) (result u32) (canon lift (core func $i "run"))))
            WAT);
        $exports = (new Instance($component, [
            'docs:demo/slow' => ['later' => new Suspending(fn (int $n): int => \Fiber::suspend() + $n)],
        ]))->exports;

        $fiber = new \Fiber(fn (): int => $exports->run(2));
        $fiber->start();
        $fiber->resume(40);

        self::assertSame(42, $fiber->getReturn());
    }

    public function test_wasi_start_on_a_parked_instance_is_busy(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (import "later" (func $later (result u32)))
              (core func $later-core (canon lower (func $later)))
              (core module $m
                (import "host" "later" (func $later (result i32)))
                (func (export "wait") (result i32) (call $later))
                (func (export "run") (result i32) (i32.const 0)))
              (core instance $i (instantiate $m (with "host" (instance (export "later" (func $later-core))))))
              (func (export "wait") (result u32) (canon lift (core func $i "wait")))
              (func $run (result (result)) (canon lift (core func $i "run")))
              (instance $run-instance (export "run" (func $run)))
              (export "wasi:cli/run@0.2.0" (instance $run-instance)))
            WAT);
        $wasi = new \Wasm\Wasi();
        $instance = new Instance($component, ['later' => new Suspending(fn (): int => \Fiber::suspend())], $wasi);
        $fiber = new \Fiber(fn (): int => $instance->exports->wait());
        $fiber->start();

        try {
            $wasi->start($instance);
            self::fail('Expected the store to be busy');
        } catch (RuntimeError $busy) {
            self::assertSame('the store is busy with a suspended call', $busy->getMessage());
        }
        $fiber->resume(3);
        self::assertSame(3, $fiber->getReturn());
    }

    public function test_resource_functions_of_a_parked_instance_are_busy(): void
    {
        $exports = (new Instance(new Component(self::RESOURCES), ['later' => new Suspending(fn (): int => \Fiber::suspend())]))->exports;
        $ready = new \Fiber(fn () => $exports->get('thing')->new());
        $ready->start();
        $ready->resume(1);
        $thing = $ready->getReturn();
        $waiting = new \Fiber(fn () => $exports->get('thing')->new());
        $waiting->start();

        foreach ([fn () => $exports->get('thing')->new(), fn () => $thing->value()] as $call) {
            try {
                $call();
                self::fail('Expected the store to be busy');
            } catch (RuntimeError $busy) {
                self::assertSame('the store is busy with a suspended call', $busy->getMessage());
            }
        }

        // Dropped while the other call waits: released once the store is free again.
        $thing->drop();
        $waiting->resume(2);
        self::assertSame(2, $waiting->getReturn()->value());
    }

    public function test_wasi_components_resumed_out_of_order_do_not_panic(): void
    {
        $script = <<<'PHP'
            <?php
            $component = new Wasm\Component\Component('(component
              (import "wasi:clocks/monotonic-clock@0.2.0" (instance $clock (export "now" (func (result u64)))))
              (alias export $clock "now" (func $now))
              (import "later" (func $later (result u32)))
              (core func $now-core (canon lower (func $now)))
              (core func $later-core (canon lower (func $later)))
              (core module $m
                (import "host" "now" (func $now (result i64)))
                (import "host" "later" (func $later (result i32)))
                (func (export "run") (result i32) (drop (call $now)) (call $later)))
              (core instance $i (instantiate $m (with "host" (instance
                (export "now" (func $now-core)) (export "later" (func $later-core))))))
              (func (export "run") (result u32) (canon lift (core func $i "run"))))');
            $fibers = [];
            foreach (['a', 'b'] as $name) {
                $exports = (new Wasm\Component\Instance($component, [
                    'later' => new Wasm\Suspending(fn (): int => Fiber::suspend()),
                ], new Wasm\Wasi()))->exports;
                $fibers[$name] = new Fiber(fn (): int => $exports->run());
                $fibers[$name]->start();
            }
            $fibers['a']->resume(1);
            $fibers['b']->resume(2);
            echo $fibers['a']->getReturn(), ',', $fibers['b']->getReturn();
            PHP;

        $output = $this->runPhp($script, $exitCode);

        self::assertSame(0, $exitCode, $output);
        self::assertSame('1,2', $output);
    }
}
