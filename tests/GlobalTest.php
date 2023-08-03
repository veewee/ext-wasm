<?php

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\InstanceBuilder;

class GlobalTest extends TestCase
{
    public function test_it_can_read_and_write_globals() {
        $instance = $this->createBuilderFromWat()->build();

        self::assertSame(0, $instance->some);
        
        $instance->some = 1;
        self::assertSame(1, $instance->some);

        $return = $instance->set_some(21);
        self::assertNull($return);
        self::assertSame(21, $instance->some);
        self::assertSame([21], $instance->get_some());
    }

    public function test_it_can_not_change_immutable_globals() {
        $instance = $this->createBuilderFromWat()->build();

        self::assertSame(1, $instance->one);
        
        $this->expectException(\Exception::class);
        $instance->one = 2;
    }

    private function createBuilderFromWat(?string $wat = null): InstanceBuilder
    {
        return InstanceBuilder::fromWat($wat ?? <<<'EOWAT'
            (module
                (global $one (export "one") i32 (i32.const 1))
                (global $some (export "some") (mut i32) (i32.const 0))
                (func (export "get_one") (result i32) (global.get $one))
                (func (export "get_some") (result i32) (global.get $some))
                (func (export "set_some") (param i32) (global.set $some (local.get 0)))
            )
            EOWAT);
    }
}
