<?php

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\InstanceBuilder;
use Wasm\WasmInstance;

class InstanceTest extends TestCase
{
    public function test_instance_cannot_be_constructed() {
        $this->expectException(\Exception::class);
        new WasmInstance();
    }

    public function test_instance_builder_cannot_be_constructed() {
        $this->expectException(\Exception::class);
        new InstanceBuilder();
    }

    public function test_instance_can_be_built_from_builder() {
        $instance = $this->createBuilderFromWat()->build();
        self::assertInstanceOf(WasmInstance::class, $instance);
    }

    public function test_instance_can_be_built_from_instance() {
        $instance = WasmInstance::fromBuilder($this->createBuilderFromWat());
        self::assertInstanceOf(WasmInstance::class, $instance);
    }

    public function test_it_can_build_multiple_instances_from_builder() {
        $instance1 = $this->createBuilderFromWat()->build();
        $instance2 = $this->createBuilderFromWat()->build();

        self::assertInstanceOf(WasmInstance::class, $instance1);
        self::assertInstanceOf(WasmInstance::class, $instance2);

        self::assertSame([33], $instance1->add_one(32));
    }

    public function test_instance_cannot_be_build_on_invalid_wat() {
        $this->expectException(\Exception::class);
        WasmInstance::fromBuilder($this->createBuilderFromWat('(module INVALIDWAT'));
    }

    private function createBuilderFromWat(?string $wat = null): InstanceBuilder
    {
        return InstanceBuilder::fromWat($wat ?? <<<'EOWAT'
            (module
              (type $t0 (func (param i32) (result i32)))
              (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
                get_local $p0
                i32.const 1
                i32.add))
            EOWAT);
    }
}
