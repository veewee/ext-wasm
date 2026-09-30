<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Exception\LinkError;
use Wasm\Instance;
use Wasm\Memory;
use Wasm\Module;

/**
 * wasm.memory_limit, taken by a store when it is created.
 */
final class LimitsTest extends TestCase
{
    private const GROW = <<<'WAT'
        (module
          (memory (export "memory") 1)
          (table $t 0 funcref)
          (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0)))
          (func (export "growTable") (param i32) (result i32) (table.grow $t (ref.null func) (local.get 0))))
        WAT;

    protected function tearDown(): void
    {
        ini_restore('wasm.memory_limit');
    }

    private static function grow(): Instance
    {
        return new Instance(new Module(self::GROW));
    }

    public function test_memory_cannot_grow_past_the_limit(): void
    {
        ini_set('wasm.memory_limit', '1M');
        $exports = self::grow()->exports;

        self::assertSame(1, $exports->grow(10));
        self::assertSame(-1, $exports->grow(10));
        self::assertSame(11, $exports->grow(5));
    }

    public function test_a_grow_refused_by_the_memory_maximum_uses_no_budget(): void
    {
        ini_set('wasm.memory_limit', '1M');
        $exports = (new Instance(new Module('(module (memory 1 2) (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0))))')))->exports;

        for ($i = 0; $i < 20; ++$i) {
            self::assertSame(-1, $exports->grow(5));
        }
        self::assertSame(1, $exports->grow(1));
    }

    public function test_a_table_cannot_grow_past_the_limit(): void
    {
        ini_set('wasm.memory_limit', '1M');
        $exports = self::grow()->exports;

        self::assertSame(-1, $exports->growTable(200_000));
        self::assertSame(0, $exports->growTable(1000));
    }

    public function test_a_table_grow_that_overflows_keeps_what_was_granted(): void
    {
        ini_set('wasm.memory_limit', '1M');
        $exports = (new Instance(new Module(<<<'WAT'
            (module
              (table $t i64 1 funcref)
              (func (export "grow") (param i64) (result i64) (table.grow $t (ref.null func) (local.get 0))))
            WAT)))->exports;

        self::assertSame(1, $exports->grow(100_000));
        self::assertSame(-1, $exports->grow(-1));
        self::assertSame(-1, $exports->grow(100_000));
    }

    public function test_a_failed_instantiation_gives_its_memory_back_to_the_store(): void
    {
        ini_set('wasm.memory_limit', '1M');
        $store = new \Wasm\Store();

        try {
            new Instance(new Module('(module (memory 10) (memory 10))'), store: $store);
            self::fail('both memories fit');
        } catch (LinkError) {
        }
        self::assertInstanceOf(Instance::class, new Instance(new Module('(module (memory 10))'), store: $store));
    }

    public function test_an_initial_memory_over_the_limit_is_a_link_error(): void
    {
        ini_set('wasm.memory_limit', '1M');

        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('exceed wasm.memory_limit');
        new Instance(new Module('(module (memory 32))'));
    }

    public function test_a_standalone_memory_over_the_limit_throws(): void
    {
        ini_set('wasm.memory_limit', '1M');

        $this->expectExceptionMessage('exceed wasm.memory_limit');
        new Memory(['initial' => 32]);
    }

    public function test_growing_a_memory_from_php_past_the_limit_throws(): void
    {
        ini_set('wasm.memory_limit', '1M');
        $memory = self::grow()->exports->memory;

        $this->expectExceptionMessage('exceed wasm.memory_limit');
        $memory->grow(32);
    }

    public function test_the_limit_is_taken_when_the_store_is_created(): void
    {
        $exports = self::grow()->exports;
        ini_set('wasm.memory_limit', '1M');

        self::assertSame(1, $exports->grow(32));
    }

    public function test_standalone_objects_get_a_new_store_when_the_limit_changed(): void
    {
        $unlimited = new Memory(['initial' => 1]);
        ini_set('wasm.memory_limit', '1M');
        $limited = new Memory(['initial' => 1]);
        ini_restore('wasm.memory_limit');
        $again = new Memory(['initial' => 1]);

        self::assertSame(1, $unlimited->grow(32));
        self::assertSame(1, $again->grow(32));
        $this->expectExceptionMessage('exceed wasm.memory_limit');
        $limited->grow(32);
    }

    public function test_a_component_cannot_grow_past_the_limit(): void
    {
        ini_set('wasm.memory_limit', '1M');
        $component = new Component(<<<'WAT'
            (component
              (core module $m
                (memory 1)
                (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0))))
              (core instance $i (instantiate $m))
              (func (export "grow") (param "pages" s32) (result s32) (canon lift (core func $i "grow"))))
            WAT);
        $exports = (new \Wasm\Component\Instance($component))->exports;

        self::assertSame(1, $exports->grow(10));
        self::assertSame(-1, $exports->grow(10));
    }

    public function test_invalid_values_are_rejected(): void
    {
        self::assertFalse(ini_set('wasm.memory_limit', 'lots'));
        self::assertFalse(ini_set('wasm.memory_limit', '12X'));
        self::assertFalse(ini_set('wasm.memory_limit', '-5'));
        self::assertSame('0', ini_get('wasm.memory_limit'));
        self::assertNotFalse(ini_set('wasm.memory_limit', '-1'));
        self::assertNotFalse(ini_set('wasm.memory_limit', '64M'));
        self::assertSame('64M', ini_get('wasm.memory_limit'));
    }
}
