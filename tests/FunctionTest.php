<?php

namespace Test;

use PHPUnit\Framework\TestCase;

class FunctionTest extends TestCase
{
    public function test_it_fails_on_invalid_function_call() {
        $instance = \Wasm\InstanceBuilder::fromWat(
            <<<'EOWAT'
            (module
              (type $t0 (func (param i32) (result i32)))
              (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
                get_local $p0
                i32.const 1
                i32.add))
            EOWAT
        )->build();

        $this->expectException(\Exception::class);
        self::assertNull($instance->add_two(2));
    }

    public function test_it_fails_on_invalid_argument() {
        $instance = \Wasm\InstanceBuilder::fromWat(
            <<<'EOWAT'
            (module
              (type $t0 (func (param i32) (result i32)))
              (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
                get_local $p0
                i32.const 1
                i32.add))
            EOWAT
        )->build();

        $this->expectException(\Exception::class);
        self::assertNull($instance->add_one('invalid'));
    }

    public function test_it_fails_on_invalid_argument_count() {
        $instance = \Wasm\InstanceBuilder::fromWat(
            <<<'EOWAT'
            (module
              (type $t0 (func (param i32) (result i32)))
              (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
                get_local $p0
                i32.const 1
                i32.add))
            EOWAT
        )->build();

        $this->expectException(\Exception::class);
        self::assertNull($instance->add_one(1, 2));
    }

    public function test_it_can_call_void_return_function() {
        $instance = \Wasm\InstanceBuilder::fromWat(
            <<<'EOWAT'
            (module
                (func (export "do_nothing")
                nop
                )
            )
            EOWAT
        )->build();

        self::assertNull($instance->do_nothing());
    }

    public function test_it_can_call_single_return_function() {
        $instance = \Wasm\InstanceBuilder::fromWat(
            <<<'EOWAT'
            (module
              (type $t0 (func (param i32) (result i32)))
              (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
                get_local $p0
                i32.const 1
                i32.add))
            EOWAT
        )->build();

        self::assertSame([43], $instance->add_one(42));
    }

    public function test_it_can_call_multi_return_function() {
        $instance = \Wasm\InstanceBuilder::fromWat(
            <<<'EOWAT'
              (module
                (func $swap (export "swap") (param i32 i32) (result i32 i32)
                  (local.get 1) (local.get 0)
                )
              )
              EOWAT
        )->build();

        self::assertSame([2, 1], $instance->swap(1, 2));
    }
}
