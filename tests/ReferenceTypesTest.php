<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Func;
use Wasm\GlobalVar;
use Wasm\Instance;
use Wasm\Module;

final class ReferenceTypesTest extends TestCase
{
    public function test_externref_round_trips_any_php_value_by_identity(): void
    {
        $exports = $this->exports('(func (export "id") (param externref) (result externref) local.get 0)');
        $object = new \stdClass();

        self::assertSame($object, $exports->id($object));
        self::assertSame([1, 'two'], $exports->id([1, 'two']));
        self::assertSame('text', $exports->id('text'));
        self::assertNull($exports->id(null));
    }

    public function test_externref_passes_through_php_callbacks(): void
    {
        $seen = null;
        $object = new \ArrayObject();
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "see" (func $see (param externref)))
              (func (export "run") (param externref) (call $see (local.get 0))))
            EOWAT), ['env' => ['see' => function ($value) use (&$seen): void {
            $seen = $value;
        }]]))->exports;

        $exports->run($object);

        self::assertSame($object, $seen);
    }

    public function test_externref_globals(): void
    {
        $object = new \stdClass();
        $global = new GlobalVar(['value' => 'externref', 'mutable' => true], $object);

        self::assertSame($object, $global->value);

        $global->value = null;
        self::assertNull($global->value);
    }

    public function test_is_null_on_externref(): void
    {
        $exports = $this->exports('(func (export "is_null") (param externref) (result i32) (ref.is_null (local.get 0)))');

        self::assertSame(1, $exports->is_null(null));
        self::assertSame(0, $exports->is_null(false));
    }

    public function test_values_held_only_by_wasm_are_released_after_collection(): void
    {
        $exports = $this->exports('(func (export "id") (param externref) (result externref) local.get 0)');
        $object = new \stdClass();
        $weak = \WeakReference::create($object);
        $exports->id($object);
        unset($object);

        // Enough externrefs to cross the collection threshold.
        for ($i = 0; $i < 5000; $i++) {
            $exports->id($i);
        }

        self::assertNull($weak->get());
    }

    public function test_funcref_round_trips(): void
    {
        $exports = $this->exports(<<<'EOWAT'
            (func $answer (export "answer") (result i32) (i32.const 42))
            (elem declare func $answer)
            (func (export "get") (result funcref) (ref.func $answer))
            (func (export "id") (param funcref) (result funcref) local.get 0)
            EOWAT);

        $func = $exports->get();
        self::assertInstanceOf(Func::class, $func);
        self::assertSame(42, $func());
        self::assertSame(42, ($exports->id($exports->answer))());
        self::assertNull($exports->id(null));
    }

    public function test_funcref_rejects_php_callables(): void
    {
        $exports = $this->exports('(func (export "id") (param funcref) (result funcref) local.get 0)');

        $this->expectException(\TypeError::class);
        $exports->id(fn () => 1);
    }

    public function test_v128_is_a_16_byte_string(): void
    {
        $exports = $this->exports(<<<'EOWAT'
            (func (export "id") (param v128) (result v128) local.get 0)
            (func (export "add") (param v128 v128) (result v128) (i32x4.add (local.get 0) (local.get 1)))
            EOWAT);
        $bytes = pack('V4', 1, 2, 3, 4);

        self::assertSame($bytes, $exports->id($bytes));
        self::assertSame(pack('V4', 2, 4, 6, 8), $exports->add($bytes, $bytes));
    }

    public function test_v128_requires_exactly_16_bytes(): void
    {
        $exports = $this->exports('(func (export "id") (param v128) (result v128) local.get 0)');

        $this->expectException(\TypeError::class);
        $exports->id('short');
    }

    public function test_v128_globals(): void
    {
        $global = new GlobalVar(['value' => 'v128'], str_repeat("\x01", 16));

        self::assertSame(str_repeat("\x01", 16), $global->value);
        self::assertSame(str_repeat("\0", 16), (new GlobalVar(['value' => 'v128']))->value);
    }

    private function exports(string $body): \Wasm\Exports
    {
        return (new Instance(new Module("(module $body)")))->exports;
    }
}
