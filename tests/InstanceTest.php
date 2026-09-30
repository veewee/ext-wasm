<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exports;
use Wasm\Func;
use Wasm\Instance;
use Wasm\Module;

final class InstanceTest extends TestCase
{
    private const WAT = <<<'EOWAT'
        (module
          (func (export "add_one") (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add))
        EOWAT;

    public function test_it_instantiates_a_module(): void
    {
        $instance = new Instance(new Module(self::WAT));

        self::assertInstanceOf(Exports::class, $instance->exports);
    }

    public function test_exports_are_funcs(): void
    {
        $instance = new Instance(new Module(self::WAT));

        self::assertInstanceOf(Func::class, $instance->exports->add_one);
        self::assertTrue(isset($instance->exports->add_one));
        self::assertFalse(isset($instance->exports->unknown));
    }

    public function test_it_builds_multiple_instances_from_one_module(): void
    {
        $module = new Module(self::WAT);
        $one = new Instance($module);
        $two = new Instance($module);

        self::assertSame(33, $one->exports->add_one(32));
        self::assertSame(34, $two->exports->add_one(33));
    }

    public function test_unknown_export_throws(): void
    {
        $instance = new Instance(new Module(self::WAT));

        $this->expectException(\Error::class);
        $instance->exports->unknown;
    }

    public function test_exports_are_iterable_in_module_order(): void
    {
        $instance = new Instance(new Module(<<<'EOWAT'
            (module
              (func (export "b"))
              (func (export "a")))
            EOWAT));

        self::assertSame(['b', 'a'], array_keys(iterator_to_array($instance->exports)));
    }
}
