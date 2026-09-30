<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Func;
use Wasm\Component\Instance;

final class ComponentCallTest extends TestCase
{
    /** Core functions that return their argument, by core type. */
    private const CORE = <<<'WAT'
        (core module $m
          (memory (export "memory") 1)
          (global $next (mut i32) (i32.const 1024))
          (func (export "realloc") (param i32 i32 i32 i32) (result i32)
            (local $p i32)
            (local.set $p (global.get $next))
            (global.set $next (i32.add (local.get $p) (local.get 3)))
            (local.get $p))
          (func (export "i32") (param i32) (result i32) (local.get 0))
          (func (export "i64") (param i64) (result i64) (local.get 0))
          (func (export "f32") (param f32) (result f32) (local.get 0))
          (func (export "f64") (param f64) (result f64) (local.get 0))
          (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1)))
          ;; Returns the string it was given, as a pointer to a (ptr, len) pair.
          (func (export "echo") (param i32 i32) (result i32)
            (i32.store (i32.const 0) (local.get 0))
            (i32.store (i32.const 4) (local.get 1))
            (i32.const 0)))
        (core instance $i (instantiate $m))
        WAT;

    private static function exports(): Exports
    {
        $lifts = '';
        foreach (['bool', 's8', 'u8', 's16', 'u16', 's32', 'u32', 'char'] as $type) {
            $lifts .= "(func (export \"id-$type\") (param \"v\" $type) (result $type) (canon lift (core func \$i \"i32\")))\n";
        }
        foreach (['s64', 'u64'] as $type) {
            $lifts .= "(func (export \"id-$type\") (param \"v\" $type) (result $type) (canon lift (core func \$i \"i64\")))\n";
        }
        $lifts .= '(func (export "id-f32") (param "v" f32) (result f32) (canon lift (core func $i "f32")))' . "\n";
        $lifts .= '(func (export "id-f64") (param "v" f64) (result f64) (canon lift (core func $i "f64")))' . "\n";

        $component = new Component('(component ' . self::CORE . $lifts . <<<'WAT'
            (func (export "echo") (param "s" string) (result string)
              (canon lift (core func $i "echo") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
            (func $add (param "a" u32) (param "b" u32) (result u32) (canon lift (core func $i "add")))
            (export "add-numbers" (func $add))
            (export "get-URL" (func $add))
            (export "v2-parse" (func $add))
            (instance $math (export "add" (func $add)))
            (export "docs:demo/math@0.1.0" (instance $math)))
            WAT);

        return (new Instance($component))->exports;
    }

    /** @return iterable<string, array{string, mixed}> */
    public static function roundTrips(): iterable
    {
        yield 'bool true' => ['idBool', true];
        yield 'bool false' => ['idBool', false];
        yield 's8 min' => ['idS8', -128];
        yield 's8 max' => ['idS8', 127];
        yield 'u8 max' => ['idU8', 255];
        yield 's16 min' => ['idS16', -32768];
        yield 'u16 max' => ['idU16', 65535];
        yield 's32 min' => ['idS32', -2147483648];
        yield 'u32 max' => ['idU32', 4294967295];
        yield 's64 min' => ['idS64', PHP_INT_MIN];
        yield 's64 max' => ['idS64', PHP_INT_MAX];
        yield 'u64 max as its bit pattern' => ['idU64', -1];
        yield 'u64 in range' => ['idU64', PHP_INT_MAX];
        yield 'f32' => ['idF32', 1.5];
        yield 'f64' => ['idF64', 0.1];
        yield 'char ascii' => ['idChar', 'a'];
        yield 'char multibyte' => ['idChar', "\u{1F600}"];
        yield 'string' => ['echo', 'hello, wörld'];
        yield 'empty string' => ['echo', ''];
    }

    #[DataProvider('roundTrips')]
    public function test_values_round_trip(string $function, mixed $value): void
    {
        self::assertSame($value, self::exports()->{$function}($value));
    }

    public function test_an_int_is_accepted_for_a_float(): void
    {
        self::assertSame(2.0, self::exports()->idF64(2));
    }

    /** @return iterable<string, array{string, mixed, class-string<\Throwable>}> */
    public static function rejected(): iterable
    {
        yield 'u8 above range' => ['idU8', 256, \ValueError::class];
        yield 's8 below range' => ['idS8', -129, \ValueError::class];
        yield 'u32 negative' => ['idU32', -1, \ValueError::class];
        yield 'string for an int' => ['idU32', '1', \TypeError::class];
        yield 'float for an int' => ['idS32', 1.5, \TypeError::class];
        yield 'int for a bool' => ['idBool', 1, \TypeError::class];
        yield 'two characters for a char' => ['idChar', 'ab', \ValueError::class];
        yield 'empty string for a char' => ['idChar', '', \ValueError::class];
        yield 'invalid UTF-8' => ['echo', "\xff", \ValueError::class];
        yield 'int for a string' => ['echo', 1, \TypeError::class];
    }

    /** @param class-string<\Throwable> $error */
    #[DataProvider('rejected')]
    public function test_values_are_checked(string $function, mixed $value, string $error): void
    {
        $this->expectException($error);
        self::exports()->{$function}($value);
    }

    public function test_functions_are_called_by_their_camel_case_name(): void
    {
        self::assertSame(5, self::exports()->addNumbers(2, 3));
    }

    public function test_camel_case_of_unusual_names(): void
    {
        $exports = self::exports();

        self::assertSame(3, $exports->getUrl(1, 2));
        self::assertSame(3, $exports->v2Parse(1, 2));
    }

    public function test_get_returns_a_callable_function_by_its_wit_name(): void
    {
        $add = self::exports()->get('add-numbers');

        self::assertInstanceOf(Func::class, $add);
        self::assertSame(7, $add(3, 4));
    }

    public function test_get_returns_an_exported_interface_with_or_without_version(): void
    {
        $exports = self::exports();

        self::assertSame(3, $exports->get('docs:demo/math')->add(1, 2));
        self::assertSame(3, $exports->get('docs:demo/math@0.1.0')->add(1, 2));
    }

    public function test_an_unknown_export_is_an_error(): void
    {
        $this->expectException(\Error::class);
        $this->expectExceptionMessage('component has no export named "nope"');
        self::exports()->get('nope');
    }

    public function test_an_unknown_method_is_an_error(): void
    {
        $this->expectException(\Error::class);
        self::exports()->nope();
    }

    public function test_exports_iterate_by_wit_name(): void
    {
        $names = array_keys(iterator_to_array(self::exports()));

        self::assertContains('add-numbers', $names);
        self::assertContains('docs:demo/math@0.1.0', $names);
        self::assertInstanceOf(Exports::class, iterator_to_array(self::exports())['docs:demo/math@0.1.0']);
    }

    public function test_the_argument_count_is_checked(): void
    {
        $this->expectException(\ArgumentCountError::class);
        self::exports()->addNumbers(1);
    }
}
