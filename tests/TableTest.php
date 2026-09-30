<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\RuntimeError;
use Wasm\Func;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Table;

final class TableTest extends TestCase
{
    public function test_it_creates_a_funcref_table(): void
    {
        $table = new Table(['element' => 'anyfunc', 'initial' => 2, 'maximum' => 4]);

        self::assertSame(2, $table->length());
        self::assertNull($table->get(0));
    }

    public function test_it_creates_an_externref_table_with_initial_value(): void
    {
        $object = new \stdClass();
        $table = new Table(['element' => 'externref', 'initial' => 3], $object);

        self::assertSame($object, $table->get(2));
    }

    public function test_it_sets_gets_and_grows(): void
    {
        $table = new Table(['element' => 'externref', 'initial' => 1, 'maximum' => 3]);

        $table->set(0, 'hello');
        self::assertSame('hello', $table->get(0));

        self::assertSame(1, $table->grow(2, 42));
        self::assertSame(3, $table->length());
        self::assertSame(42, $table->get(2));

        $table->set(0);
        self::assertNull($table->get(0));
    }

    public function test_growing_past_maximum_throws(): void
    {
        $table = new Table(['element' => 'anyfunc', 'initial' => 1, 'maximum' => 1]);

        $this->expectException(\ValueError::class);
        $table->grow(1);
    }

    public function test_out_of_bounds_access_throws(): void
    {
        $table = new Table(['element' => 'anyfunc', 'initial' => 1]);

        $this->expectException(\ValueError::class);
        $table->get(1);
    }

    public function test_funcref_tables_only_accept_funcs(): void
    {
        $table = new Table(['element' => 'anyfunc', 'initial' => 1]);

        $this->expectException(\TypeError::class);
        $table->set(0, 'strlen');
    }

    public function test_unknown_element_type_throws(): void
    {
        $this->expectException(\TypeError::class);
        new Table(['element' => 'i32', 'initial' => 1]);
    }

    public function test_wasm_calls_funcs_placed_in_an_imported_table(): void
    {
        $math = (new Instance(new Module(<<<'EOWAT'
            (module
              (func (export "double") (param i32) (result i32) (i32.mul (local.get 0) (i32.const 2)))
              (func (export "square") (param i32) (result i32) (i32.mul (local.get 0) (local.get 0))))
            EOWAT)))->exports;
        $table = new Table(['element' => 'anyfunc', 'initial' => 2]);
        $table->set(0, $math->double);
        $table->set(1, $math->square);

        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "table" (table 2 funcref))
              (type $unary (func (param i32) (result i32)))
              (func (export "apply") (param i32 i32) (result i32)
                (call_indirect (type $unary) (local.get 1) (local.get 0))))
            EOWAT), ['env' => ['table' => $table]]))->exports;

        self::assertSame(14, $exports->apply(0, 7));
        self::assertSame(49, $exports->apply(1, 7));
    }

    public function test_exported_table_entries_are_callable_funcs(): void
    {
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (table (export "table") 1 funcref)
              (func $seven (result i32) (i32.const 7))
              (elem (i32.const 0) $seven))
            EOWAT)))->exports;

        self::assertInstanceOf(Table::class, $exports->table);
        $func = $exports->table->get(0);
        self::assertInstanceOf(Func::class, $func);
        self::assertSame(7, $func());
    }

    public function test_calling_a_null_entry_traps(): void
    {
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (table 1 funcref)
              (func (export "run") (call_indirect (i32.const 0))))
            EOWAT)))->exports;

        $this->expectException(RuntimeError::class);
        $exports->run();
    }
}
