<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Exception\RuntimeError;
use Wasm\Suspending;
use Wasm\Wasi;

/**
 * The async component model: `async func` exports and imports, streams and
 * futures, on wasmtime's concurrent event loop.
 */
final class ComponentAsyncTest extends TestCase
{
    /** An async export run() that returns plain() + 1, through task.return. */
    private const ASYNC_EXPORT = <<<'WAT'
        (component
          (import "plain" (func $plain (result u32)))
          (core func $plain-core (canon lower (func $plain)))
          (core func $task-return (canon task.return (result u32)))
          (core module $m
            (import "host" "plain" (func $plain (result i32)))
            (import "host" "task-return" (func $ret (param i32)))
            (func (export "run") (result i32) (call $ret (i32.add (call $plain) (i32.const 1))) (i32.const 0))
            (func (export "cb") (param i32 i32 i32) (result i32) unreachable))
          (core instance $i (instantiate $m (with "host" (instance
            (export "plain" (func $plain-core))
            (export "task-return" (func $task-return))))))
          (func (export "run") async (result u32) (canon lift (core func $i "run") async (callback (core func $i "cb")))))
        WAT;

    /** tests/fixtures/component-async, with `slow` implemented by $slow. */
    private static function demo(callable|Suspending $slow): Exports
    {
        return (new Instance(
            Component::fromFile(__DIR__ . '/fixtures/component-async/component-async.wasm'),
            ['slow' => $slow],
            new Wasi(),
        ))->exports;
    }

    public function test_an_async_import_is_a_php_callable(): void
    {
        self::assertSame(41, self::demo(fn (int $n): int => $n * 10)->run(4));
    }

    public function test_two_async_import_calls_can_be_in_flight_at_once(): void
    {
        $calls = [];
        $exports = self::demo(function (int $n) use (&$calls): int {
            $calls[] = $n;

            return $n * 10;
        });

        self::assertSame(30, $exports->both());
        self::assertSame([1, 2], $calls);
    }

    public function test_a_suspending_async_import_suspends_its_fiber(): void
    {
        $exports = self::demo(new Suspending(fn (int $n): int => \Fiber::suspend($n) * 10));

        $fiber = new \Fiber(fn (): int => $exports->run(4));
        self::assertSame(4, $fiber->start());
        $fiber->resume(5);

        self::assertSame(51, $fiber->getReturn());
    }

    public function test_two_suspending_calls_in_flight_resume_one_after_the_other(): void
    {
        $exports = self::demo(new Suspending(fn (int $n): int => \Fiber::suspend($n)));

        $fiber = new \Fiber(fn (): int => $exports->both());
        $first = $fiber->start();
        $second = $fiber->resume(100);
        $fiber->resume(200);

        self::assertSame([1, 2], [$first, $second]);
        self::assertSame(300, $fiber->getReturn());
    }

    public function test_an_async_import_cannot_call_back_into_its_instance(): void
    {
        $exports = null;
        $error = null;
        $exports = self::demo(function (int $n) use (&$exports, &$error): int {
            try {
                $exports->run(1);
            } catch (RuntimeError $e) {
                $error = $e->getMessage();
            }

            return $n;
        });

        self::assertSame(5, $exports->run(4));
        self::assertSame('the store is busy with a suspended call', $error);
    }

    public function test_an_async_component_stays_usable_after_an_import_throws(): void
    {
        $throw = true;
        $exports = self::demo(function (int $n) use (&$throw): int {
            if ($throw) {
                throw new \RuntimeException('boom');
            }

            return $n;
        });

        try {
            $exports->run(1);
            self::fail('the import did not throw');
        } catch (\RuntimeException $e) {
            self::assertSame('boom', $e->getMessage());
        }
        $throw = false;
        self::assertSame(3, $exports->run(2));
    }

    public function test_an_async_component_stays_usable_after_its_fiber_is_destroyed_mid_call(): void
    {
        $exports = self::demo(new Suspending(fn (int $n): int => \Fiber::suspend() + $n));
        $fiber = new \Fiber(fn (): int => $exports->run(1));
        $fiber->start();
        unset($fiber);
        gc_collect_cycles();

        $again = new \Fiber(fn (): int => $exports->run(2));
        $again->start();
        $again->resume(5);
        self::assertSame(8, $again->getReturn());
    }

    public function test_an_async_export_with_a_plain_import_returns_its_result(): void
    {
        $exports = (new Instance(new Component(self::ASYNC_EXPORT), ['plain' => fn (): int => 41]))->exports;

        self::assertSame(42, $exports->run());
        self::assertSame(42, $exports->run());
    }

    public function test_a_plain_import_of_an_async_component_may_not_switch_fibers(): void
    {
        $exports = (new Instance(new Component(self::ASYNC_EXPORT), ['plain' => fn (): int => \Fiber::suspend()]))->exports;

        $this->expectException(\FiberError::class);
        (new \Fiber(fn (): int => $exports->run()))->start();
    }

    public function test_the_signature_of_an_async_export_says_async(): void
    {
        $component = new Component(self::ASYNC_EXPORT);

        self::assertSame('async func() -> u32', $component->exports()[0]['type'] ?? null);
    }
}
