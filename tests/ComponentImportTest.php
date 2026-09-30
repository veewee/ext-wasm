<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Component\Result;
use Wasm\Exception\ComponentError;
use Wasm\Exception\LinkError;
use Wasm\Exception\RuntimeError;

final class ComponentImportTest extends TestCase
{
    /**
     * Imports now() at the world level and an interface with format() and
     * check(), and exports functions that call them, plus outer(), which
     * calls the callback() import, and inner().
     */
    private const COMPONENT = <<<'WAT'
        (component
          (import "now" (func $now (result u64)))
          (import "docs:demo/names@0.1.0" (instance $names
            (export "format" (func (param "name" string) (result string)))
            (export "check" (func (param "n" u32) (result (result u32 (error string)))))))
          (import "callback" (func $callback (result u32)))
          (alias export $names "format" (func $format))
          (alias export $names "check" (func $check))

          (core module $libc
            (memory (export "memory") 1)
            (global $next (mut i32) (i32.const 1024))
            (func (export "realloc") (param i32 i32 i32 i32) (result i32)
              (local $p i32)
              (local.set $p (i32.and (i32.add (global.get $next) (i32.const 7)) (i32.const -8)))
              (global.set $next (i32.add (local.get $p) (local.get 3)))
              (local.get $p)))
          (core instance $libc (instantiate $libc))
          (alias core export $libc "memory" (core memory $mem))
          (alias core export $libc "realloc" (core func $realloc))

          (core func $now-core (canon lower (func $now)))
          (core func $format-core (canon lower (func $format) (memory $mem) (realloc $realloc)))
          (core func $check-core (canon lower (func $check) (memory $mem) (realloc $realloc)))
          (core func $callback-core (canon lower (func $callback)))

          (core module $main
            (import "host" "now" (func $now (result i64)))
            (import "host" "format" (func $format (param i32 i32 i32)))
            (import "host" "check" (func $check (param i32 i32)))
            (import "host" "callback" (func $callback (result i32)))
            (func (export "call-now") (result i64) (call $now))
            (func (export "greet") (param i32 i32) (result i32)
              (call $format (local.get 0) (local.get 1) (i32.const 16))
              (i32.const 16))
            (func (export "run-check") (param i32) (result i32)
              (call $check (local.get 0) (i32.const 32))
              (i32.const 32))
            (func (export "outer") (result i32) (call $callback))
            (func (export "inner") (result i32) (i32.const 7)))
          (core instance $host
            (export "now" (func $now-core))
            (export "format" (func $format-core))
            (export "check" (func $check-core))
            (export "callback" (func $callback-core)))
          (core instance $i (instantiate $main (with "host" (instance $host))))

          (func (export "call-now") (result u64) (canon lift (core func $i "call-now")))
          (func (export "greet") (param "name" string) (result string)
            (canon lift (core func $i "greet") (memory $mem) (realloc $realloc)))
          (func (export "run-check") (param "n" u32) (result (result u32 (error string)))
            (canon lift (core func $i "run-check") (memory $mem) (realloc $realloc)))
          (func (export "outer") (result u32) (canon lift (core func $i "outer")))
          (func (export "inner") (result u32) (canon lift (core func $i "inner"))))
        WAT;

    /** @param array<string, mixed> $overrides */
    private static function instance(array $overrides = [], ?Exports &$exports = null): Exports
    {
        $imports = array_replace([
            'now' => fn (): int => 42,
            'docs:demo/names' => [
                'format' => fn (string $name): string => "Hello, $name",
                'check' => fn (int $n): int => $n,
            ],
            'callback' => fn (): int => 1,
        ], $overrides);
        $exports = (new Instance(new Component(self::COMPONENT), $imports))->exports;

        return $exports;
    }

    public function test_a_world_level_import_is_a_php_callable(): void
    {
        self::assertSame(42, self::instance()->callNow());
    }

    public function test_an_interface_import_is_keyed_without_version(): void
    {
        self::assertSame('Hello, Ada', self::instance()->greet('Ada'));
    }

    public function test_an_interface_import_may_also_use_its_versioned_name(): void
    {
        $exports = self::instance([
            'docs:demo/names@0.1.0' => ['format' => fn (string $n): string => "Hi $n", 'check' => fn (int $n): int => $n],
            'docs:demo/names' => null,
        ]);

        self::assertSame('Hi Ada', $exports->greet('Ada'));
    }

    public function test_a_missing_interface_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('docs:demo/names@0.1.0');
        self::instance(['docs:demo/names' => null]);
    }

    public function test_a_missing_function_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('check');
        self::instance(['docs:demo/names' => ['format' => fn (string $n): string => $n]]);
    }

    public function test_a_non_callable_import_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        self::instance(['now' => 42]);
    }

    public function test_a_function_the_interface_does_not_declare_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('other');
        self::instance(['docs:demo/names' => [
            'format' => fn (string $n): string => $n,
            'check' => fn (int $n): int => $n,
            'other' => fn () => null,
        ]]);
    }

    public function test_a_core_func_is_not_a_component_import(): void
    {
        $core = (new \Wasm\Instance(new \Wasm\Module('(module (func (export "f") (result i64) (i64.const 1)))')))->exports->f;

        $this->expectException(LinkError::class);
        self::instance(['now' => $core]);
    }

    public function test_an_import_signals_an_err_by_throwing_a_component_error(): void
    {
        $exports = self::instance(['docs:demo/names' => [
            'format' => fn (string $n): string => $n,
            'check' => fn (int $n): int => $n > 0 ? $n : throw new ComponentError('too small'),
        ]]);

        self::assertSame(3, $exports->runCheck(3));
        try {
            $exports->runCheck(0);
            self::fail('Expected a ComponentError');
        } catch (ComponentError $error) {
            self::assertSame('too small', $error->payload);
        }
        self::assertSame(4, $exports->runCheck(4));
    }

    public function test_an_import_may_return_a_result(): void
    {
        $exports = self::instance(['docs:demo/names' => [
            'format' => fn (string $n): string => $n,
            'check' => fn (int $n): Result => $n > 0 ? Result::ok($n * 2) : Result::err('nope'),
        ]]);

        self::assertSame(6, $exports->runCheck(3));
        $this->expectException(ComponentError::class);
        $exports->runCheck(0);
    }

    public function test_another_exception_reaches_the_caller_and_poisons_the_instance(): void
    {
        $thrown = new \DomainException('from php');
        $exports = self::instance(['now' => fn (): int => throw $thrown]);

        try {
            $exports->callNow();
            self::fail('Expected the import exception');
        } catch (\DomainException $caught) {
            self::assertSame($thrown, $caught);
        }

        // The component model marks an instance as trapped after a failed call.
        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('cannot enter component instance');
        $exports->greet('Ada');
    }

    public function test_a_fresh_instance_works_after_one_was_poisoned(): void
    {
        $exports = self::instance(['now' => fn (): int => throw new \DomainException()]);
        try {
            $exports->callNow();
        } catch (\DomainException) {
        }

        self::assertSame('Hello, Ada', self::instance()->greet('Ada'));
    }

    public function test_a_wrong_return_type_traps(): void
    {
        $exports = self::instance(['now' => fn (): string => 'soon']);

        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('expected int');
        $exports->callNow();
    }

    public function test_an_argument_error_does_not_poison_the_instance(): void
    {
        $exports = self::instance();
        try {
            $exports->greet(5);
            self::fail('Expected a TypeError');
        } catch (\TypeError) {
        }

        self::assertSame('Hello, Ada', $exports->greet('Ada'));
    }

    public function test_an_import_may_call_back_into_its_own_instance(): void
    {
        $exports = null;
        $exports = self::instance(['callback' => function () use (&$exports): int {
            return $exports->inner() + 1;
        }]);

        self::assertSame(8, $exports->outer());
    }

    public function test_an_import_cannot_switch_fibers(): void
    {
        $exports = self::instance(['now' => fn (): int => \Fiber::suspend()]);

        $this->expectException(\FiberError::class);
        (new \Fiber(fn () => $exports->callNow()))->start();
    }
}
