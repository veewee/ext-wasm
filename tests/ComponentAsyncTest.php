<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Component\Stream;
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

    public function test_a_stream_returned_by_a_call_is_read_after_the_call_returned(): void
    {
        $stream = self::demo(fn (int $n): int => $n)->countUp(5);

        self::assertInstanceOf(Stream::class, $stream);
        $read = [];
        while (($chunk = $stream->read()) !== null) {
            $read = [...$read, ...$chunk];
        }
        self::assertSame([0, 1, 2, 3, 4], $read);
        self::assertNull($stream->read());
    }

    public function test_a_stream_is_iterable_by_chunk(): void
    {
        $chunks = [];
        foreach (self::demo(fn (int $n): int => $n)->words() as $chunk) {
            $chunks[] = $chunk;
        }

        self::assertSame(['alpha', 'beta', 'gamma'], array_merge(...$chunks));
    }

    public function test_a_byte_stream_gives_binary_string_chunks(): void
    {
        $body = '';
        foreach (self::demo(fn (int $n): int => $n)->bytes(10) as $chunk) {
            self::assertIsString($chunk);
            $body .= $chunk;
        }

        self::assertSame(str_repeat('a', 10), $body);
    }

    public function test_an_endless_stream_can_be_read_partly_and_dropped(): void
    {
        $exports = self::demo(fn (int $n): int => $n);
        $stream = $exports->endless();

        self::assertSame(str_repeat('x', 16), $stream->read());
        self::assertSame(str_repeat('x', 16), $stream->read());
        unset($stream);

        self::assertSame(8, $exports->run(7));
    }

    public function test_a_stream_dropped_unread_does_not_block_the_instance(): void
    {
        $exports = self::demo(fn (int $n): int => $n);
        $exports->endless();
        $exports->countUp(3);

        self::assertSame(8, $exports->run(7));
    }

    public function test_reading_a_stream_that_never_progresses_throws_instead_of_hanging(): void
    {
        $stream = self::demo(fn (int $n): int => $n)->stuck();

        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('the component cannot make progress');
        $stream->read();
    }

    public function test_an_array_is_a_stream_for_the_component(): void
    {
        $exports = self::demo(fn (int $n): int => $n);

        self::assertSame(6, $exports->sum([1, 2, 3]));
        self::assertSame(5, $exports->length(['ab', 'cde']));
        self::assertSame(0, $exports->sum([]));
    }

    public function test_a_generator_is_read_lazily_by_the_component(): void
    {
        $exports = self::demo(fn (int $n): int => $n);
        $produced = 0;
        $numbers = (function () use (&$produced) {
            for ($i = 1; $i <= 100; ++$i) {
                ++$produced;
                yield $i;
            }
        })();

        self::assertSame(5050, $exports->sum($numbers));
        self::assertSame(100, $produced);
        self::assertSame(1 << 16, $exports->length((function () {
            for ($i = 0; $i < 16; ++$i) {
                yield str_repeat('z', 4096);
            }
        })()));
    }

    public function test_an_iterator_aggregate_is_a_stream_too(): void
    {
        $words = new \ArrayObject([1, 2, 3, 4]);

        self::assertSame(10, self::demo(fn (int $n): int => $n)->sum($words));
    }

    public function test_an_exception_from_a_generator_reaches_the_caller(): void
    {
        $exports = self::demo(fn (int $n): int => $n);

        $this->expectException(\LogicException::class);
        $this->expectExceptionMessage('no more numbers');
        $exports->sum((function () {
            yield 1;
            throw new \LogicException('no more numbers');
        })());
    }

    public function test_a_value_of_the_wrong_type_in_a_stream_throws(): void
    {
        $exports = self::demo(fn (int $n): int => $n);

        $this->expectException(\TypeError::class);
        $exports->sum((function () {
            yield 1;
            yield 'two';
        })());
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
