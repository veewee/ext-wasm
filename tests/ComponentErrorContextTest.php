<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\ErrorContext;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Exception\ComponentError;
use Wasm\Exception\LinkError;

/**
 * WIT error-context as an opaque Wasm\Component\ErrorContext. wasmtime 49
 * gives the host no access to its debug message and does not count the
 * host's references, so PHP receives one but cannot pass one to a component.
 */
final class ComponentErrorContextTest extends TestCase
{
    private const COMPONENT = <<<'WAT'
        (component
          (import "report" (func $report (param "e" error-context)))
          (core module $libc (memory (export "memory") 1) (data (i32.const 0) "boom"))
          (core instance $libc (instantiate $libc))
          (alias core export $libc "memory" (core memory $mem))
          (core func $new (canon error-context.new (memory $mem) string-encoding=utf8))
          (core func $drop (canon error-context.drop))
          (core func $report-core (canon lower (func $report)))
          (core module $m
            (import "host" "memory" (memory 1))
            (import "host" "new" (func $new (param i32 i32) (result i32)))
            (import "host" "report" (func $report (param i32)))
            (import "host" "drop" (func $drop (param i32)))
            ;; the guest keeps every handle it made until forget drops them
            (global $made (mut i32) (i32.const 0))
            (func $boom (result i32)
              (local $e i32)
              (local.set $e (call $new (i32.const 0) (i32.const 4)))
              (global.set $made (local.get $e))
              (local.get $e))
            (func (export "forget")
              (local $e i32)
              (local.set $e (i32.const 1))
              (block $done (loop $next
                (br_if $done (i32.gt_u (local.get $e) (global.get $made)))
                (call $drop (local.get $e))
                (local.set $e (i32.add (local.get $e) (i32.const 1)))
                (br $next))))
            (func (export "make") (result i32) (call $boom))
            (func (export "fail") (result i32)
              (i32.store (i32.const 100) (i32.const 1))
              (i32.store (i32.const 104) (call $boom))
              (i32.const 100))
            (func (export "many") (result i32)
              (i32.store (i32.const 200) (call $boom))
              (i32.store (i32.const 204) (call $boom))
              (i32.store (i32.const 100) (i32.const 200))
              (i32.store (i32.const 104) (i32.const 2))
              (i32.const 100))
            (func (export "take") (param i32) (result i32) (i32.const 7))
            (func (export "maybe") (param i32 i32) (result i32) (i32.eqz (local.get 0)))
            (func (export "run") (call $report (call $boom))))
          (core instance $i (instantiate $m (with "host" (instance
            (export "memory" (memory $mem))
            (export "new" (func $new))
            (export "report" (func $report-core))
            (export "drop" (func $drop))))))
          (func (export "make") (result error-context) (canon lift (core func $i "make")))
          (func (export "fail") (result (result u32 (error error-context))) (canon lift (core func $i "fail") (memory $mem)))
          (func (export "many") (result (list error-context)) (canon lift (core func $i "many") (memory $mem)))
          (func (export "take") (param "e" error-context) (result u32) (canon lift (core func $i "take")))
          (func (export "maybe") (param "e" (option error-context)) (result bool) (canon lift (core func $i "maybe")))
          (func (export "run") (canon lift (core func $i "run")))
          (func (export "forget") (canon lift (core func $i "forget"))))
        WAT;

    /** @param \Closure(ErrorContext): void|null $report */
    private static function exports(?\Closure $report = null): Exports
    {
        return (new Instance(new Component(self::COMPONENT), [
            'report' => $report ?? static fn (ErrorContext $e) => null,
        ]))->exports;
    }

    public function test_an_error_context_comes_out_as_an_object(): void
    {
        self::assertInstanceOf(ErrorContext::class, self::exports()->make());
    }

    public function test_an_err_error_context_is_the_thrown_payload(): void
    {
        try {
            self::exports()->fail();
            self::fail('the err was not thrown');
        } catch (ComponentError $e) {
            self::assertInstanceOf(ErrorContext::class, $e->payload);
        }
    }

    public function test_error_contexts_come_out_inside_other_values(): void
    {
        $many = self::exports()->many();

        self::assertCount(2, $many);
        self::assertContainsOnlyInstancesOf(ErrorContext::class, $many);
        self::assertNotSame($many[0], $many[1]);
    }

    public function test_a_php_import_receives_an_error_context(): void
    {
        $received = null;
        self::exports(static function (ErrorContext $e) use (&$received): void {
            $received = $e;
        })->run();

        self::assertInstanceOf(ErrorContext::class, $received);
    }

    public function test_php_keeps_an_error_context_the_component_dropped(): void
    {
        $exports = self::exports();
        $kept = $exports->make();
        $exports->run();
        $exports->forget();

        self::assertInstanceOf(ErrorContext::class, $exports->make());
        unset($exports);
        gc_collect_cycles();
        self::assertInstanceOf(ErrorContext::class, $kept);
    }

    public function test_an_error_context_cannot_be_passed_back(): void
    {
        $exports = self::exports();
        $given = $exports->make();

        try {
            $exports->take($given);
            self::fail('the error-context was passed back');
        } catch (\TypeError $e) {
            self::assertSame(
                'a component cannot be given an error-context yet, got Wasm\Component\ErrorContext',
                $e->getMessage(),
            );
        }
        self::assertTrue($exports->maybe(null));
    }

    public function test_no_other_value_stands_in_for_an_error_context(): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('got int');

        self::exports()->take(1);
    }

    public function test_an_option_error_context_takes_null(): void
    {
        self::assertTrue(self::exports()->maybe(null));
    }

    public function test_an_import_returning_an_error_context_is_a_link_error(): void
    {
        $component = new Component('(component (import "give" (func (result error-context))))');

        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('error-context');

        new Instance($component, ['give' => static fn () => null]);
    }

    public function test_an_import_returning_an_optional_error_context_is_a_link_error(): void
    {
        $component = new Component('(component (import "give" (func (result (option error-context)))))');

        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('error-context');

        new Instance($component, ['give' => static fn () => null]);
    }

    public function test_an_import_parameter_still_refuses_streams_of_error_contexts(): void
    {
        $component = new Component('(component (import "take" (func (param "e" (list (stream error-context))))))');

        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('stream<error-context>');

        new Instance($component, ['take' => static fn (array $e) => null]);
    }

    public function test_php_cannot_create_an_error_context(): void
    {
        $this->expectException(\Exception::class);
        $this->expectExceptionMessage('You cannot instantiate this class from PHP.');

        new ErrorContext();
    }

    public function test_error_contexts_are_reflected(): void
    {
        $component = new Component(self::COMPONENT);
        $exports = array_column($component->exports(), null, 'name');
        $imports = array_column($component->imports(), null, 'name');

        self::assertSame('func(e: error-context) -> u32', $exports['take']['type']);
        self::assertSame('error-context', $exports['take']['signature']->params['e']->kind);
        self::assertSame('error-context', $exports['make']['signature']->result->kind);
        self::assertSame('error-context', $imports['report']['signature']->params['e']->kind);
    }
}
