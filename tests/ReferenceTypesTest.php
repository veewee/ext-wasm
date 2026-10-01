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

    private const TYPED = <<<'EOWAT'
        (type $unary (func (param i32) (result i32)))
        (func $double (export "double") (type $unary) (i32.mul (local.get 0) (i32.const 2)))
        (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1)))
        (func (export "apply") (param $f (ref null $unary)) (param $x i32) (result i32)
          (if (result i32) (ref.is_null (local.get $f))
            (then (i32.const -1))
            (else (call_ref $unary (local.get $x) (local.get $f)))))
        (func (export "applyStrict") (param $f (ref $unary)) (param $x i32) (result i32)
          (call_ref $unary (local.get $x) (local.get $f)))
        (table (export "unaries") 1 (ref null $unary))
        (global (export "current") (mut (ref null $unary)) (ref.null $unary))
        (elem declare func $double)
        EOWAT;

    public function test_a_function_reference_of_a_module_type_takes_a_matching_func(): void
    {
        $exports = $this->exports(self::TYPED);

        self::assertSame(42, $exports->apply($exports->double, 21));
        self::assertSame(42, $exports->applyStrict($exports->double, 21));
    }

    public function test_null_fits_only_a_nullable_function_reference(): void
    {
        $exports = $this->exports(self::TYPED);
        self::assertSame(-1, $exports->apply(null, 21));

        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('needs a value, it cannot be null');
        $exports->applyStrict(null, 21);
    }

    public function test_a_func_of_another_type_is_a_type_error_naming_both(): void
    {
        $exports = $this->exports(self::TYPED);

        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('expected a Wasm\Func of type (i32) -> (i32), got one of type (i32, i32) -> (i32)');

        $exports->apply($exports->add, 21);
    }

    public function test_a_declared_subtype_fits_its_supertype(): void
    {
        $exports = $this->exports(<<<'EOWAT'
            (type $base (sub (func (result i32))))
            (type $derived (sub $base (func (result i32))))
            (func (export "seven") (type $derived) (i32.const 7))
            (func (export "call") (param (ref $base)) (result i32) (call_ref $base (local.get 0)))
            (elem declare func 0)
            EOWAT);

        self::assertSame(7, $exports->call($exports->seven));
    }

    public function test_the_same_type_from_another_module_fits(): void
    {
        $store = new \Wasm\Store();
        $caller = new Instance(new Module(<<<'EOWAT'
            (module
              (type $t (func (result i32)))
              (func (export "call") (param (ref $t)) (result i32) (call_ref $t (local.get 0))))
            EOWAT), store: $store);
        $callee = new Instance(new Module('(module (type $t (func (result i32))) (func (export "nine") (type $t) (i32.const 9)))'), store: $store);

        self::assertSame(9, $caller->exports->call($callee->exports->nine));
    }

    public function test_a_lookalike_type_from_another_rec_group_does_not_fit(): void
    {
        $exports = $this->exports(<<<'EOWAT'
            (type $t (func (result i32)))
            (rec (type $other (func (result i32))) (type $pair (struct)))
            (func (export "lookalike") (type $other) (i32.const 1))
            (func (export "call") (param (ref $t)) (result i32) (call_ref $t (local.get 0)))
            EOWAT);

        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('expected a Wasm\Func of type () -> (i32)');

        $exports->call($exports->lookalike);
    }

    public function test_a_php_import_may_return_a_function_reference(): void
    {
        $module = new Module(<<<'EOWAT'
            (module
              (type $t (func (result i32)))
              (import "env" "pick" (func $pick (param i32) (result (ref null $t))))
              (func (export "ten") (type $t) (i32.const 10))
              (func (export "other") (param i32) (result i32) (local.get 0))
              (func (export "run") (param i32) (result i32) (call_ref $t (call $pick (local.get 0))))
              (elem declare func 0))
            EOWAT);
        $exports = null;
        $instance = new Instance($module, ['env' => ['pick' => function (int $which) use (&$exports): \Wasm\Func {
            return $which === 0 ? $exports->ten : $exports->other;
        }]]);
        $exports = $instance->exports;

        self::assertSame(10, $exports->run(0));

        $this->expectExceptionMessage('expected a Wasm\Func of type () -> (i32), got one of type (i32) -> (i32)');
        $exports->run(1);
    }

    public function test_tables_and_globals_of_a_module_function_type_take_a_matching_func(): void
    {
        $exports = $this->exports(self::TYPED);

        $exports->unaries->set(0, $exports->double);
        self::assertSame(42, $exports->unaries->get(0)(21));

        $exports->current->value = $exports->double;
        self::assertSame(42, ($exports->current->value)(21));
    }

    private function exports(string $body): \Wasm\Exports
    {
        return (new Instance(new Module("(module $body)")))->exports;
    }
}
