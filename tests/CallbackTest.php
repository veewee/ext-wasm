<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\LinkError;
use Wasm\Exception\RuntimeError;
use Wasm\Instance;
use Wasm\Module;

final class CallbackTest extends TestCase
{
    public function test_it_calls_a_php_closure_with_converted_arguments(): void
    {
        $received = [];
        $exports = $this->instance(
            '(import "env" "log" (func $log (param i32 i64 f32 f64)))
             (func (export "run") (call $log (i32.const -1) (i64.const 2) (f32.const 1.5) (f64.const 0.25)))',
            ['env' => ['log' => function (int $a, int $b, float $c, float $d) use (&$received): void {
                $received = [$a, $b, $c, $d];
            }]],
        );

        self::assertNull($exports->run());
        self::assertSame([-1, 2, 1.5, 0.25], $received);
    }

    public function test_it_uses_the_callback_result(): void
    {
        $exports = $this->instance(
            '(import "env" "double" (func $double (param i32) (result i32)))
             (func (export "run") (param i32) (result i32) (i32.add (call $double (local.get 0)) (i32.const 1)))',
            ['env' => ['double' => fn (int $x): int => $x * 2]],
        );

        self::assertSame(21, $exports->run(10));
    }

    public function test_it_accepts_any_php_callable(): void
    {
        $exports = $this->instance(
            '(import "php" "abs" (func $abs (param i32) (result i32)))
             (func (export "run") (param i32) (result i32) (call $abs (local.get 0)))',
            ['php' => ['abs' => 'abs']],
        );

        self::assertSame(5, $exports->run(-5));
    }

    public function test_it_maps_a_list_to_multiple_results(): void
    {
        $exports = $this->instance(
            '(import "env" "pair" (func $pair (result i32 f64)))
             (func (export "run") (result i32 f64) (call $pair))',
            ['env' => ['pair' => fn (): array => [7, 0.5]]],
        );

        self::assertSame([7, 0.5], $exports->run());
    }

    public function test_a_wrong_return_type_traps(): void
    {
        $exports = $this->instance(
            '(import "env" "f" (func $f (result i32)))
             (func (export "run") (result i32) (call $f))',
            ['env' => ['f' => fn () => 'nope']],
        );

        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessageMatches('/expected int/');
        $exports->run();
    }

    public function test_it_rethrows_the_original_php_exception(): void
    {
        $thrown = new \DomainException('from php');
        $exports = $this->instance(
            '(import "env" "f" (func $f))
             (func (export "run") (call $f))',
            ['env' => ['f' => function () use ($thrown): void {
                throw $thrown;
            }]],
        );

        try {
            $exports->run();
            self::fail('Expected the callback exception to propagate');
        } catch (\DomainException $caught) {
            self::assertSame($thrown, $caught);
        }

        // The instance stays usable after the unwind.
        $this->expectException(\DomainException::class);
        $exports->run();
    }

    public function test_callbacks_can_reenter_the_instance(): void
    {
        $instance = null;
        $trace = [];
        $instance = $this->instanceObject(
            '(import "env" "outer" (func $outer (param i32) (result i32)))
             (import "env" "inner" (func $inner (param i32) (result i32)))
             (func (export "start") (param i32) (result i32) (call $outer (local.get 0)))
             (func (export "middle") (param i32) (result i32) (call $inner (i32.add (local.get 0) (i32.const 1))))
             (func (export "leaf") (param i32) (result i32) (i32.mul (local.get 0) (i32.const 10)))',
            ['env' => [
                'outer' => function (int $x) use (&$instance, &$trace): int {
                    $trace[] = "outer($x)";
                    return $instance->exports->middle($x);
                },
                'inner' => function (int $x) use (&$instance, &$trace): int {
                    $trace[] = "inner($x)";
                    return $instance->exports->leaf($x);
                },
            ]],
        );

        self::assertSame(20, $instance->exports->start(1));
        self::assertSame(['outer(1)', 'inner(2)'], $trace);
    }

    public function test_callbacks_can_read_memory_during_the_call(): void
    {
        $instance = null;
        $seen = null;
        $instance = $this->instanceObject(
            '(import "env" "print" (func $print (param i32 i32)))
             (memory (export "memory") 1)
             (data (i32.const 8) "hello")
             (func (export "run") (call $print (i32.const 8) (i32.const 5)))',
            ['env' => ['print' => function (int $ptr, int $len) use (&$instance, &$seen): void {
                $seen = $instance->exports->memory->read($ptr, $len);
                $instance->exports->memory->write($ptr, 'J');
            }]],
        );

        $instance->exports->run();

        self::assertSame('hello', $seen);
        self::assertSame('Jello', $instance->exports->memory->read(8, 5));
    }

    public function test_recursion_through_php_and_wasm(): void
    {
        $instance = null;
        $instance = $this->instanceObject(
            '(import "env" "down" (func $down (param i32) (result i32)))
             (func (export "count") (param i32) (result i32)
               (if (result i32) (i32.eqz (local.get 0))
                 (then (i32.const 0))
                 (else (i32.add (i32.const 1) (call $down (i32.sub (local.get 0) (i32.const 1)))))))',
            ['env' => ['down' => function (int $n) use (&$instance): int {
                return $instance->exports->count($n);
            }]],
        );

        self::assertSame(25, $instance->exports->count(25));
    }

    public function test_runaway_recursion_through_php_is_a_trap_not_a_crash(): void
    {
        $instance = null;
        $instance = $this->instanceObject(
            '(import "env" "again" (func $again))
             (func (export "run") (call $again))',
            ['env' => ['again' => function () use (&$instance): void {
                $instance->exports->run();
            }]],
        );

        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessageMatches('/call stack exhausted/');
        $instance->exports->run();
    }

    public function test_a_trap_in_a_nested_call_surfaces_as_runtime_error(): void
    {
        $instance = null;
        $instance = $this->instanceObject(
            '(import "env" "f" (func $f))
             (func (export "run") (call $f))
             (func (export "boom") unreachable)',
            ['env' => ['f' => function () use (&$instance): void {
                $instance->exports->boom();
            }]],
        );

        $this->expectException(RuntimeError::class);
        $instance->exports->run();
    }

    public function test_exported_funcs_can_be_imported_elsewhere(): void
    {
        $math = new Instance(new Module('(module (func (export "inc") (param i32) (result i32) (i32.add (local.get 0) (i32.const 1))))'));
        $exports = $this->instance(
            '(import "math" "inc" (func $inc (param i32) (result i32)))
             (func (export "run") (param i32) (result i32) (call $inc (call $inc (local.get 0))))',
            ['math' => ['inc' => $math->exports->inc]],
        );

        self::assertSame(3, $exports->run(1));
    }

    public function test_a_non_callable_function_import_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        $this->instance('(import "env" "f" (func))', ['env' => ['f' => 'not a function']]);
    }

    public function test_destructors_of_values_released_by_a_callback_can_use_wasm(): void
    {
        $memory = new \Wasm\Memory(['initial' => 1]);
        $probe = new \ArrayObject();
        $exports = $this->instance(
            '(import "env" "f" (func $f)) (func (export "run") (call $f))',
            // The returned object is discarded because the import has no results.
            ['env' => ['f' => fn () => new class ($memory, $probe) {
                public function __construct(private \Wasm\Memory $memory, private \ArrayObject $probe)
                {
                }

                public function __destruct()
                {
                    $this->probe['seen'] = $this->memory->byteLength();
                }
            }]],
        );

        $exports->run();

        self::assertSame(65536, $probe['seen'] ?? null);
    }

    public function test_start_function_can_call_php(): void
    {
        $called = false;
        $this->instance(
            '(import "env" "f" (func $f)) (start $f)',
            ['env' => ['f' => function () use (&$called): void {
                $called = true;
            }]],
        );

        self::assertTrue($called);
    }

    private function instance(string $body, array $imports): \Wasm\Exports
    {
        return $this->instanceObject($body, $imports)->exports;
    }

    private function instanceObject(string $body, array $imports): Instance
    {
        return new Instance(new Module("(module $body)"), $imports);
    }
}
