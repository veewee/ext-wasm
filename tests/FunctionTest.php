<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\RuntimeError;
use Wasm\Exports;
use Wasm\Instance;
use Wasm\Module;

final class FunctionTest extends TestCase
{
    public function test_it_calls_void_function(): void
    {
        self::assertNull($this->exports('(func (export "f") nop)')->f());
    }

    public function test_it_returns_a_single_result_as_value(): void
    {
        self::assertSame(43, $this->exports(self::ADD_ONE)->add_one(42));
    }

    public function test_it_returns_multiple_results_as_list(): void
    {
        $exports = $this->exports('(func (export "swap") (param i32 i32) (result i32 i32) (local.get 1) (local.get 0))');

        self::assertSame([2, 1], $exports->swap(1, 2));
    }

    public function test_func_is_invokable(): void
    {
        $add = $this->exports(self::ADD_ONE)->add_one;

        self::assertSame(2, $add(1));
        self::assertSame(1, $add->length());
    }

    public function test_i32_accepts_unsigned_range_and_returns_signed(): void
    {
        $exports = $this->exports('(func (export "id") (param i32) (result i32) local.get 0)');

        self::assertSame(-1, $exports->id(0xFFFFFFFF));
        self::assertSame(-2147483648, $exports->id(-2147483648));
    }

    public function test_i32_out_of_range_throws_value_error(): void
    {
        $exports = $this->exports('(func (export "id") (param i32) (result i32) local.get 0)');

        $this->expectException(\ValueError::class);
        $exports->id(0x100000000);
    }

    public function test_i64_roundtrips_full_range(): void
    {
        $exports = $this->exports('(func (export "id") (param i64) (result i64) local.get 0)');

        self::assertSame(PHP_INT_MAX, $exports->id(PHP_INT_MAX));
        self::assertSame(PHP_INT_MIN, $exports->id(PHP_INT_MIN));
    }

    public function test_floats_accept_int_and_float(): void
    {
        $exports = $this->exports(<<<'EOWAT'
            (func (export "f32") (param f32) (result f32) local.get 0)
            (func (export "f64") (param f64) (result f64) local.get 0)
            EOWAT);

        self::assertSame(1.5, $exports->f32(1.5));
        self::assertSame(2.0, $exports->f32(2));
        self::assertSame(0.1, $exports->f64(0.1));
        self::assertSame(3.0, $exports->f64(3));
    }

    public function test_it_rejects_strings_for_numbers(): void
    {
        $this->expectException(\TypeError::class);
        $this->exports(self::ADD_ONE)->add_one('1');
    }

    public function test_it_rejects_floats_for_ints(): void
    {
        $this->expectException(\TypeError::class);
        $this->exports(self::ADD_ONE)->add_one(1.0);
    }

    public function test_it_rejects_wrong_argument_count(): void
    {
        $this->expectException(\ArgumentCountError::class);
        $this->exports(self::ADD_ONE)->add_one(1, 2);
    }

    public function test_it_throws_on_unknown_function(): void
    {
        $this->expectException(\Error::class);
        $this->exports(self::ADD_ONE)->add_two(1);
    }

    public function test_trap_throws_runtime_error(): void
    {
        $exports = $this->exports('(func (export "boom") unreachable)');

        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessageMatches('/unreachable/');
        $exports->boom();
    }

    public function test_instance_survives_trap(): void
    {
        $exports = $this->exports('(func (export "boom") unreachable)' . self::ADD_ONE);

        try {
            $exports->boom();
        } catch (RuntimeError) {
        }

        self::assertSame(2, $exports->add_one(1));
    }

    private const ADD_ONE = '(func (export "add_one") (param i32) (result i32) local.get 0 i32.const 1 i32.add)';

    private function exports(string $body): Exports
    {
        return (new Instance(new Module("(module $body)")))->exports;
    }
}
