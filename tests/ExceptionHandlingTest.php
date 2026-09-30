<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\RuntimeError;
use Wasm\Exception\WasmException;
use Wasm\Exception\WasmThrow;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Tag;

final class ExceptionHandlingTest extends TestCase
{
    public function test_tags_are_created_exported_and_imported(): void
    {
        $tag = new Tag(['parameters' => ['i32', 'f64']]);
        $instance = new Instance(new Module('(module (import "env" "e" (tag (param i32 f64))) (export "same" (tag 0)))'), [
            'env' => ['e' => $tag],
        ]);

        self::assertInstanceOf(Tag::class, $instance->exports->same);
    }

    public function test_tag_parameters_must_be_value_types(): void
    {
        $this->expectException(\TypeError::class);
        new Tag(['parameters' => ['nope']]);
    }

    public function test_an_uncaught_wasm_exception_becomes_a_wasm_throw(): void
    {
        $exports = $this->exports(<<<'EOWAT'
            (tag $e (export "e") (param i32 f64))
            (func (export "run") (throw $e (i32.const 42) (f64.const 0.5)))
            EOWAT);

        $line = __LINE__ + 2;
        try {
            $exports->run();
            self::fail('Expected a WasmThrow');
        } catch (WasmThrow $thrown) {
            self::assertInstanceOf(WasmException::class, $thrown);
            self::assertSame($exports->e, $thrown->tag);
            self::assertSame([42, 0.5], $thrown->payload);
            self::assertSame($line, $thrown->getLine());
        }
    }

    public function test_a_php_created_tag_keeps_its_identity(): void
    {
        $tag = new Tag(['parameters' => ['i32']]);
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "e" (tag $e (param i32)))
              (func (export "run") (throw $e (i32.const 1))))
            EOWAT), ['env' => ['e' => $tag]]))->exports;

        try {
            $exports->run();
            self::fail('Expected a WasmThrow');
        } catch (WasmThrow $thrown) {
            self::assertSame($tag, $thrown->tag);
        }
    }

    public function test_wasm_catches_a_wasm_throw_thrown_by_php(): void
    {
        $tag = new Tag(['parameters' => ['i32']]);
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
            EOWAT), ['env' => ['e' => $tag, 'fail' => function () use ($tag): void {
            throw new WasmThrow($tag, [41]);
        }]]))->exports;

        self::assertSame(42, $exports->run());
    }

    public function test_an_uncaught_php_wasm_throw_comes_back_out(): void
    {
        $tag = new Tag(['parameters' => ['externref']]);
        $object = new \stdClass();
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "fail" (func $fail))
              (func (export "run") (call $fail)))
            EOWAT), ['env' => ['fail' => function () use ($tag, $object): void {
            throw new WasmThrow($tag, [$object]);
        }]]))->exports;

        try {
            $exports->run();
            self::fail('Expected a WasmThrow');
        } catch (WasmThrow $thrown) {
            self::assertSame($tag, $thrown->tag);
            self::assertSame([$object], $thrown->payload);
        }
    }

    public function test_other_php_exceptions_are_not_catchable_by_wasm(): void
    {
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "fail" (func $fail))
              (func (export "run") (result i32)
                (block $caught
                  (try_table (catch_all $caught) (call $fail))
                  (return (i32.const 0)))
                (i32.const 1)))
            EOWAT), ['env' => ['fail' => function (): void {
            throw new \DomainException('php only');
        }]]))->exports;

        $this->expectException(\DomainException::class);
        $exports->run();
    }

    public function test_the_payload_must_match_the_tag(): void
    {
        $tag = new Tag(['parameters' => ['i32']]);

        $this->expectException(\TypeError::class);
        new WasmThrow($tag, ['not an int']);
    }

    public function test_the_payload_length_must_match_the_tag(): void
    {
        $tag = new Tag(['parameters' => ['i32', 'i32']]);

        $this->expectException(\ValueError::class);
        new WasmThrow($tag, [1]);
    }

    public function test_wasm_throw_is_a_regular_exception_in_php(): void
    {
        $tag = new Tag(['parameters' => []]);
        $line = __LINE__ + 1;
        $thrown = new WasmThrow($tag);

        self::assertSame([], $thrown->payload);
        self::assertSame($line, $thrown->getLine());
        self::assertFalse(is_subclass_of(WasmThrow::class, RuntimeError::class));
    }

    private function exports(string $body): \Wasm\Exports
    {
        return (new Instance(new Module("(module $body)")))->exports;
    }
}
