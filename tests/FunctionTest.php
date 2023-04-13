<?php

namespace Test;

use PHPUnit\Framework\TestCase;

class FunctionTest extends TestCase
{
    public function test_it_can_call_function() {
        $instance = new \Wasm\WasmInstance(
            <<<'EOWAT'
            (module
              (type $t0 (func (param i32) (result i32)))
              (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
                get_local $p0
                i32.const 1
                i32.add))
            EOWAT
        );
        
        self::assertSame(43, $instance->add_one(42));
    }
}
