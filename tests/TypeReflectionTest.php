<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Wasm\GlobalVar;
use Wasm\Instance;
use Wasm\Memory;
use Wasm\Module;
use Wasm\Table;
use Wasm\Tag;

/** Types follow the JS type reflection proposal (WebAssembly/js-types). */
final class TypeReflectionTest extends TestCase
{
    private const EXPORTS = <<<'WAT'
        (module
          (func (export "add") (param i32 i64) (result f32 f64)
            (f32.const 0) (f64.const 0))
          (func (export "nothing"))
          (global (export "counter") (mut i32) (i32.const 0))
          (global (export "pi") f64 (f64.const 3.14))
          (memory (export "bounded") 1 4)
          (table (export "functions") 2 funcref)
          (tag (export "error") (param i32 v128)))
        WAT;

    private const FUNCTION = ['parameters' => ['i32', 'i64'], 'results' => ['f32', 'f64']];

    /** @return array<string, array{name: string, kind: string, type: array<string, mixed>}> */
    private static function exportsByName(Module $module): array
    {
        return array_column($module->exports(), null, 'name');
    }

    public function test_exports_carry_their_type(): void
    {
        self::assertSame([
            ['name' => 'add', 'kind' => 'function', 'type' => self::FUNCTION],
            ['name' => 'nothing', 'kind' => 'function', 'type' => ['parameters' => [], 'results' => []]],
            ['name' => 'counter', 'kind' => 'global', 'type' => ['value' => 'i32', 'mutable' => true]],
            ['name' => 'pi', 'kind' => 'global', 'type' => ['value' => 'f64', 'mutable' => false]],
            ['name' => 'bounded', 'kind' => 'memory', 'type' => ['minimum' => 1, 'maximum' => 4]],
            ['name' => 'functions', 'kind' => 'table', 'type' => ['element' => 'funcref', 'minimum' => 2]],
            ['name' => 'error', 'kind' => 'tag', 'type' => ['parameters' => ['i32', 'v128']]],
        ], (new Module(self::EXPORTS))->exports());
    }

    public function test_imports_carry_their_type(): void
    {
        $module = new Module(<<<'WAT'
            (module
              (import "env" "log" (func (param externref) (result i32)))
              (import "env" "flag" (global i64))
              (import "env" "memory" (memory 2))
              (import "env" "table" (table 1 8 externref))
              (import "env" "error" (tag (param f32))))
            WAT);

        self::assertSame([
            ['module' => 'env', 'name' => 'log', 'kind' => 'function', 'type' => ['parameters' => ['externref'], 'results' => ['i32']]],
            ['module' => 'env', 'name' => 'flag', 'kind' => 'global', 'type' => ['value' => 'i64', 'mutable' => false]],
            ['module' => 'env', 'name' => 'memory', 'kind' => 'memory', 'type' => ['minimum' => 2]],
            ['module' => 'env', 'name' => 'table', 'kind' => 'table', 'type' => ['element' => 'externref', 'minimum' => 1, 'maximum' => 8]],
            ['module' => 'env', 'name' => 'error', 'kind' => 'tag', 'type' => ['parameters' => ['f32']]],
        ], $module->imports());
    }

    public function test_reference_types_beyond_funcref_and_externref_are_named(): void
    {
        $module = new Module(<<<'WAT'
            (module
              (type $point (struct (field i32)))
              (type $callback (func))
              (func (export "refs")
                (param funcref externref anyref eqref i31ref structref arrayref exnref nullref)
                (param (ref func) (ref null $callback) (ref $point)))
              (table (export "anys") 1 anyref))
            WAT);
        $exports = self::exportsByName($module);

        self::assertSame([
            'funcref', 'externref', 'anyref', 'eqref', 'i31ref', 'structref', 'arrayref', 'exnref', 'nullref',
            '(ref func)', '(ref null (concrete func))', '(ref (concrete struct))',
        ], $exports['refs']['type']['parameters']);
        self::assertSame(['element' => 'anyref', 'minimum' => 1], $exports['anys']['type']);
    }

    public function test_a_self_referencing_type_is_named_by_its_kind(): void
    {
        $module = new Module(<<<'WAT'
            (module
              (rec (type $node (func (param (ref null $node)))))
              (func (export "visit") (type $node))
              (tag (export "failure") (param v128 exnref)))
            WAT);
        $exports = self::exportsByName($module);

        self::assertSame(['(ref null (concrete func))'], $exports['visit']['type']['parameters']);
        self::assertSame(['v128', 'exnref'], $exports['failure']['type']['parameters']);
    }

    public function test_64_bit_memories_and_tables_carry_their_address_type(): void
    {
        $module = new Module(<<<'WAT'
            (module
              (memory (export "wide") i64 1 2)
              (table (export "huge") i64 1 18446744073709551615 funcref))
            WAT);
        $exports = self::exportsByName($module);

        self::assertSame(['minimum' => 1, 'maximum' => 2, 'address' => 'i64'], $exports['wide']['type']);
        // Beyond PHP's int range a limit is a float, as PHP's own integer overflow gives.
        self::assertSame(['element' => 'funcref', 'minimum' => 1, 'maximum' => 18446744073709551615.0, 'address' => 'i64'], $exports['huge']['type']);
    }

    public function test_a_minimum_beyond_php_int_is_reflected_as_a_float(): void
    {
        $module = new Module(<<<'WAT'
            (module
              (import "env" "huge" (table i64 9223372036854775808 funcref))
              (func (export "f")))
            WAT);

        self::assertSame(9223372036854775808.0, $module->imports()[0]['type']['minimum']);
        self::assertSame('function', $module->exports()[0]['kind']);
    }

    /** @return iterable<string, array{class-string, array<string, mixed>}> */
    public static function sizesOutOfRange(): iterable
    {
        yield '64-bit memory' => [Memory::class, ['initial' => PHP_INT_MAX, 'address' => 'i64']];
        yield '64-bit table' => [Table::class, ['element' => 'externref', 'initial' => PHP_INT_MAX, 'address' => 'i64']];
        yield '32-bit memory' => [Memory::class, ['initial' => 65537]];
        yield '32-bit table' => [Table::class, ['element' => 'externref', 'initial' => 1 << 32]];
        yield 'negative minimum' => [Memory::class, ['minimum' => -1]];
    }

    /** @param array<string, mixed> $descriptor */
    #[DataProvider('sizesOutOfRange')]
    public function test_sizes_out_of_range_are_value_errors(string $class, array $descriptor): void
    {
        $this->expectException(\ValueError::class);

        new $class($descriptor);
    }

    public function test_64_bit_host_memories_and_tables_round_trip(): void
    {
        $memory = new Memory(['minimum' => 1, 'maximum' => 3, 'address' => 'i64']);
        $table = new Table(['element' => 'externref', 'minimum' => 2, 'address' => 'i64']);

        self::assertSame(['minimum' => 1, 'maximum' => 3, 'address' => 'i64'], (new Memory($memory->type()))->type());
        self::assertSame(['element' => 'externref', 'minimum' => 2, 'address' => 'i64'], (new Table($table->type()))->type());
        self::assertSame(['minimum' => 1], (new Memory(['minimum' => 1, 'address' => 'i32']))->type());
    }

    public function test_an_unknown_address_type_is_a_type_error(): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('"address"');

        new Memory(['minimum' => 1, 'address' => 'i16']);
    }

    /** @return iterable<string, array{class-string, array<string, mixed>}> */
    public static function minimumAboveMaximum(): iterable
    {
        yield 'memory' => [Memory::class, ['initial' => 5, 'maximum' => 2]];
        yield 'table' => [Table::class, ['element' => 'funcref', 'initial' => 5, 'maximum' => 2]];
    }

    /** @param array<string, mixed> $descriptor */
    #[DataProvider('minimumAboveMaximum')]
    public function test_a_minimum_above_the_maximum_is_a_value_error(string $class, array $descriptor): void
    {
        $this->expectException(\ValueError::class);
        $this->expectExceptionMessage('maximum');

        new $class($descriptor);
    }

    public function test_a_descriptor_without_a_minimum_is_a_type_error(): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('"initial"');

        new Memory(['maximum' => 1]);
    }

    public function test_non_nullable_references_need_a_value(): void
    {
        $func = (new Instance(new Module(self::EXPORTS)))->exports->nothing;

        self::assertSame(['value' => '(ref func)', 'mutable' => false], (new GlobalVar(['value' => '(ref func)'], $func))->type());
        self::assertSame('(ref func)', (new Table(['element' => '(ref func)', 'minimum' => 1], $func))->type()['element']);

        try {
            new GlobalVar(['value' => '(ref func)']);
            self::fail('a (ref func) global without a value was created');
        } catch (\TypeError $e) {
            self::assertStringContainsString('(ref func) needs a value', $e->getMessage());
        }

        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('(ref func) needs a value');
        new Table(['element' => '(ref func)', 'minimum' => 1]);
    }

    /** @return iterable<string, array{string}> */
    public static function typesPhpCannotCreate(): iterable
    {
        yield 'anyref' => ['anyref'];
        yield 'i31ref' => ['i31ref'];
        yield 'concrete' => ['(ref null (concrete func))'];
    }

    #[DataProvider('typesPhpCannotCreate')]
    public function test_reference_types_php_cannot_create_are_type_errors(string $type): void
    {
        try {
            new GlobalVar(['value' => $type]);
            self::fail("a $type global was created");
        } catch (\TypeError $e) {
            self::assertStringContainsString("\"$type\"", $e->getMessage());
        }

        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage("\"$type\"");
        new Table(['element' => $type, 'minimum' => 1]);
    }

    public function test_null_references_round_trip(): void
    {
        self::assertSame(['value' => 'nullfuncref', 'mutable' => false], (new GlobalVar(['value' => 'nullfuncref']))->type());
        self::assertSame('nullexternref', (new Table(['element' => 'nullexternref', 'minimum' => 1]))->type()['element']);
    }

    public function test_a_null_reference_table_holds_only_null(): void
    {
        $table = new Table(['element' => 'nullexternref', 'minimum' => 1]);

        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('expected null for nullexternref, got int');

        $table->set(0, 5);
    }

    /** @return iterable<string, array{\Closure(GlobalVar, Table): void}> */
    public static function nullIntoNonNullable(): iterable
    {
        yield 'table set' => [static fn (GlobalVar $global, Table $table) => $table->set(0, null)];
        yield 'table grow' => [static fn (GlobalVar $global, Table $table) => $table->grow(1)];
        yield 'global value' => [static function (GlobalVar $global, Table $table): void {
            $global->value = null;
        }];
    }

    /** @param \Closure(GlobalVar, Table): void $write */
    #[DataProvider('nullIntoNonNullable')]
    public function test_null_never_goes_into_a_non_nullable_extern_reference(\Closure $write): void
    {
        $global = new GlobalVar(['value' => '(ref extern)', 'mutable' => true], 'held');
        $table = new Table(['element' => '(ref extern)', 'minimum' => 1], 'held');

        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('(ref extern) needs a value, it cannot be null');

        $write($global, $table);
    }

    public function test_a_function_read_back_from_a_table_reports_its_type(): void
    {
        $exports = (new Instance(new Module(self::EXPORTS)))->exports;
        $exports->functions->set(0, $exports->add);

        self::assertSame(self::FUNCTION, $exports->functions->get(0)->type());
    }

    public function test_objects_report_the_type_of_their_export(): void
    {
        $module = new Module(self::EXPORTS);
        $exports = (new Instance($module))->exports;
        $types = array_map(fn (array $export): array => $export['type'], self::exportsByName($module));

        self::assertSame($types['add'], $exports->add->type());
        self::assertSame($types['nothing'], $exports->nothing->type());
        self::assertSame($types['counter'], $exports->counter->type());
        self::assertSame($types['pi'], $exports->pi->type());
        self::assertSame($types['bounded'], $exports->bounded->type());
        self::assertSame($types['functions'], $exports->functions->type());
        self::assertSame($types['error'], $exports->error->type());
    }

    public function test_a_grown_memory_and_table_report_their_current_size(): void
    {
        $exports = (new Instance(new Module(self::EXPORTS)))->exports;
        $exports->bounded->grow(2);
        $exports->functions->grow(3);

        self::assertSame(['minimum' => 3, 'maximum' => 4], $exports->bounded->type());
        self::assertSame(['element' => 'funcref', 'minimum' => 5], $exports->functions->type());
    }

    public function test_types_of_host_objects_go_back_into_their_constructors(): void
    {
        $memory = new Memory(['initial' => 1, 'maximum' => 2]);
        $table = new Table(['element' => 'externref', 'initial' => 3]);
        $global = new GlobalVar(['value' => 'i64', 'mutable' => true], 5);
        $tag = new Tag(['parameters' => ['i32']]);

        self::assertSame(['minimum' => 1, 'maximum' => 2], (new Memory($memory->type()))->type());
        self::assertSame(['element' => 'externref', 'minimum' => 3], (new Table($table->type()))->type());
        self::assertSame(['value' => 'i64', 'mutable' => true], (new GlobalVar($global->type(), 5))->type());
        self::assertSame(['parameters' => ['i32']], (new Tag($tag->type()))->type());
    }

    public function test_minimum_and_initial_together_are_a_type_error(): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('"initial" and "minimum"');

        new Memory(['initial' => 1, 'minimum' => 1]);
    }

    public function test_a_table_rejects_minimum_and_initial_together(): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('"initial" and "minimum"');

        new Table(['element' => 'funcref', 'initial' => 1, 'minimum' => 1]);
    }
}
