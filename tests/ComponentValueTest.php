<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Component\Result;
use Wasm\Component\Variant;
use Wasm\Exception\ComponentError;
use Wasm\Exception\RuntimeError;

/**
 * Identity functions for compound WIT types. Each core function stores its
 * flattened parameters in memory in the layout of its result, which the
 * canonical ABI then lifts back, so the value makes a round trip.
 */
final class ComponentValueTest extends TestCase
{
    private const COMPONENT = <<<'WAT'
        (component
          (core module $m
            (memory (export "memory") 1)
            (data (i32.const 200) "bad")
            (global $next (mut i32) (i32.const 1024))
            (func (export "realloc") (param i32 i32 i32 i32) (result i32)
              (local $p i32)
              (local.set $p (i32.and (i32.add (global.get $next) (i32.const 7)) (i32.const -8)))
              (global.set $next (i32.add (local.get $p) (local.get 3)))
              (local.get $p))
            (func (export "i32") (param i32) (result i32) (local.get 0))
            ;; (ptr, len): a string or a list
            (func (export "pair") (param i32 i32) (result i32)
              (i32.store (i32.const 0) (local.get 0))
              (i32.store (i32.const 4) (local.get 1))
              (i32.const 0))
            ;; record { first-name: string, age: option<u8> }
            (func (export "person") (param i32 i32 i32 i32) (result i32)
              (i32.store (i32.const 0) (local.get 0))
              (i32.store (i32.const 4) (local.get 1))
              (i32.store8 (i32.const 8) (local.get 2))
              (i32.store8 (i32.const 9) (local.get 3))
              (i32.const 0))
            ;; tuple<u32, string>
            (func (export "tuple") (param i32 i32 i32) (result i32)
              (i32.store (i32.const 0) (local.get 0))
              (i32.store (i32.const 4) (local.get 1))
              (i32.store (i32.const 8) (local.get 2))
              (i32.const 0))
            ;; a discriminant with a u32 payload: variant { none, some(u32) } and option<u32>
            (func (export "tagged") (param i32 i32) (result i32)
              (i32.store8 (i32.const 0) (local.get 0))
              (i32.store (i32.const 4) (local.get 1))
              (i32.const 0))
            ;; option<option<u32>>
            (func (export "nested") (param i32 i32 i32) (result i32)
              (i32.store8 (i32.const 0) (local.get 0))
              (i32.store8 (i32.const 4) (local.get 1))
              (i32.store (i32.const 8) (local.get 2))
              (i32.const 0))
            ;; record { outcome: result<u32, string> }
            (func (export "outcome") (param i32 i32 i32) (result i32)
              (i32.store8 (i32.const 0) (local.get 0))
              (i32.store (i32.const 4) (local.get 1))
              (i32.store (i32.const 8) (local.get 2))
              (i32.const 0))
            ;; check(n) -> result<u32, string>: err "bad" for 0
            (func (export "check") (param i32) (result i32)
              (if (i32.eqz (local.get 0))
                (then
                  (i32.store8 (i32.const 0) (i32.const 1))
                  (i32.store (i32.const 4) (i32.const 200))
                  (i32.store (i32.const 8) (i32.const 3)))
                (else
                  (i32.store8 (i32.const 0) (i32.const 0))
                  (i32.store (i32.const 4) (local.get 0))))
              (i32.const 0))
            (func (export "drop") (param i32)))
          (core instance $i (instantiate $m))
          (alias core export $i "memory" (core memory $mem))
          (alias core export $i "realloc" (core func $realloc))

          (type $person' (record (field "first-name" string) (field "age" (option u8))))
          (export $person "person" (type $person'))
          (type $maybe' (variant (case "none") (case "some" u32)))
          (export $maybe "maybe" (type $maybe'))
          (type $color' (enum "red" "dark-blue"))
          (export $color "color" (type $color'))
          (type $perms' (flags "read" "write-all"))
          (export $perms "perms" (type $perms'))
          (type $outcome' (record (field "outcome" (result u32 (error string)))))
          (export $outcome "outcome" (type $outcome'))
          (type $thing' (resource (rep i32)))
          (export $thing "thing" (type $thing'))

          (func (export "id-list") (param "v" (list u32)) (result (list u32))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "id-bytes") (param "v" (list u8)) (result (list u8))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "id-strings") (param "v" (list string)) (result (list string))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "id-person") (param "v" $person) (result $person)
            (canon lift (core func $i "person") (memory $mem) (realloc $realloc)))
          (func (export "id-tuple") (param "v" (tuple u32 string)) (result (tuple u32 string))
            (canon lift (core func $i "tuple") (memory $mem) (realloc $realloc)))
          (func (export "id-maybe") (param "v" $maybe) (result $maybe)
            (canon lift (core func $i "tagged") (memory $mem) (realloc $realloc)))
          (func (export "id-option") (param "v" (option u32)) (result (option u32))
            (canon lift (core func $i "tagged") (memory $mem) (realloc $realloc)))
          (func (export "id-nested") (param "v" (option (option u32))) (result (option (option u32)))
            (canon lift (core func $i "nested") (memory $mem) (realloc $realloc)))
          (func (export "id-color") (param "v" $color) (result $color)
            (canon lift (core func $i "i32")))
          (func (export "id-perms") (param "v" $perms) (result $perms)
            (canon lift (core func $i "i32")))
          (func (export "id-outcome") (param "v" $outcome) (result $outcome)
            (canon lift (core func $i "outcome") (memory $mem) (realloc $realloc)))
          (func (export "check") (param "n" u32) (result (result u32 (error string)))
            (canon lift (core func $i "check") (memory $mem) (realloc $realloc)))
          (func (export "fails") (param "fail" bool) (result (result))
            (canon lift (core func $i "i32")))
          (func (export "take-thing") (param "t" (own $thing))
            (canon lift (core func $i "drop"))))
        WAT;

    private static function exports(): Exports
    {
        return (new Instance(new Component(self::COMPONENT)))->exports;
    }

    public function test_a_list_is_a_list_array(): void
    {
        self::assertSame([1, 2, 3], self::exports()->idList([1, 2, 3]));
        self::assertSame(['a', 'bc'], self::exports()->idStrings(['a', 'bc']));
    }

    public function test_a_list_of_bytes_is_a_binary_string(): void
    {
        self::assertSame("\x00\xffab", self::exports()->idBytes("\x00\xffab"));
        self::assertSame('', self::exports()->idBytes(''));
    }

    public function test_a_list_must_be_a_list_array(): void
    {
        $this->expectException(\TypeError::class);
        self::exports()->idList(['a' => 1]);
    }

    public function test_a_record_is_an_array_with_camel_case_keys(): void
    {
        $person = ['firstName' => 'Ada', 'age' => 36];

        self::assertSame($person, self::exports()->idPerson($person));
    }

    public function test_an_option_field_of_a_record_may_be_left_out(): void
    {
        self::assertSame(['firstName' => 'Ada', 'age' => null], self::exports()->idPerson(['firstName' => 'Ada']));
    }

    public function test_a_record_needs_every_other_field(): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('firstName');
        self::exports()->idPerson(['age' => 3]);
    }

    public function test_a_record_rejects_unknown_keys(): void
    {
        $this->expectException(\ValueError::class);
        $this->expectExceptionMessage('firstname');
        self::exports()->idPerson(['firstName' => 'Ada', 'firstname' => 'typo']);
    }

    public function test_a_tuple_is_a_list_of_exact_length(): void
    {
        self::assertSame([7, 'seven'], self::exports()->idTuple([7, 'seven']));

        $this->expectException(\TypeError::class);
        self::exports()->idTuple([7]);
    }

    public function test_a_variant_is_a_variant_object(): void
    {
        $some = self::exports()->idMaybe(new Variant('some', 5));
        $none = self::exports()->idMaybe(new Variant('none'));

        self::assertEquals(new Variant('some', 5), $some);
        self::assertSame('some', $some->tag);
        self::assertSame(5, $some->value);
        self::assertSame('none', $none->tag);
        self::assertNull($none->value);
    }

    public function test_a_variant_tag_must_exist(): void
    {
        $this->expectException(\ValueError::class);
        self::exports()->idMaybe(new Variant('many', 5));
    }

    public function test_an_enum_is_its_wit_name(): void
    {
        self::assertSame('dark-blue', self::exports()->idColor('dark-blue'));

        $this->expectException(\ValueError::class);
        self::exports()->idColor('green');
    }

    public function test_flags_are_an_array_of_booleans(): void
    {
        self::assertSame(['read' => true, 'writeAll' => false], self::exports()->idPerms(['read' => true]));
        self::assertSame(['read' => false, 'writeAll' => true], self::exports()->idPerms(['writeAll' => true, 'read' => false]));

        $this->expectException(\ValueError::class);
        self::exports()->idPerms(['execute' => true]);
    }

    public function test_an_option_is_null_or_the_value(): void
    {
        self::assertSame(4, self::exports()->idOption(4));
        self::assertNull(self::exports()->idOption(null));
    }

    public function test_an_option_in_an_option_uses_variants_inside(): void
    {
        $exports = self::exports();

        self::assertNull($exports->idNested(null));
        self::assertEquals(new Variant('none'), $exports->idNested(new Variant('none')));
        self::assertEquals(new Variant('some', 3), $exports->idNested(new Variant('some', 3)));
    }

    public function test_a_result_inside_a_value_is_a_result_object(): void
    {
        $ok = self::exports()->idOutcome(['outcome' => Result::ok(5)])['outcome'];
        $err = self::exports()->idOutcome(['outcome' => Result::err('bad')])['outcome'];

        self::assertTrue($ok->isOk());
        self::assertSame(5, $ok->value());
        self::assertTrue($err->isErr());
        self::assertSame('bad', $err->error());
    }

    public function test_a_returned_result_gives_the_ok_value(): void
    {
        self::assertSame(9, self::exports()->check(9));
        self::assertNull(self::exports()->fails(false));
    }

    public function test_a_returned_err_throws_a_component_error_with_the_payload(): void
    {
        try {
            self::exports()->check(0);
            self::fail('Expected a ComponentError');
        } catch (ComponentError $error) {
            self::assertSame('bad', $error->payload);
            self::assertSame('bad', $error->getMessage());
        }
    }

    public function test_an_err_without_payload_throws_too(): void
    {
        try {
            self::exports()->fails(true);
            self::fail('Expected a ComponentError');
        } catch (ComponentError $error) {
            self::assertNull($error->payload);
        }
    }

    public function test_result_value_of_an_err_throws_the_component_error(): void
    {
        $this->expectException(ComponentError::class);
        Result::err('nope')->value();
    }

    public function test_result_error_of_an_ok_throws(): void
    {
        $this->expectException(\Error::class);
        Result::ok(1)->error();
    }

    public function test_a_component_error_carries_any_payload(): void
    {
        $error = new ComponentError(['code' => 7]);

        self::assertSame(['code' => 7], $error->payload);
        self::assertInstanceOf(\Wasm\Exception\WasmException::class, $error);
    }

    public function test_an_unsupported_type_is_named_and_other_functions_keep_working(): void
    {
        $exports = self::exports();

        try {
            $exports->takeThing(1);
            self::fail('Expected a RuntimeError');
        } catch (RuntimeError $error) {
            self::assertStringContainsString('own<resource> is not supported yet', $error->getMessage());
        }
        self::assertSame(1, $exports->idOption(1));
    }

    public function test_variants_and_results_compare_by_value(): void
    {
        self::assertTrue(new Variant('some', 5) == new Variant('some', 5));
        self::assertFalse(new Variant('some', 5) == new Variant('some', 6));
        self::assertFalse(new Variant('some', 5) == new Variant('none'));
        self::assertTrue(Result::ok(1) == Result::ok(1));
        self::assertFalse(Result::ok(1) == Result::err(1));
        self::assertNotEquals(Result::ok([1]), Result::ok([2]));
    }

    public function test_type_errors_of_compound_types_name_the_php_type(): void
    {
        $exports = self::exports();
        foreach ([
            ['idBytes', [1, 2], 'expected string for list<u8>'],
            ['idList', 'x', 'expected list array for list<u32>'],
            ['idPerson', 'x', 'expected array for record'],
            ['idMaybe', 'x', 'expected Wasm\\Component\\Variant for variant'],
            ['idColor', 1, 'expected string for enum'],
        ] as [$function, $value, $message]) {
            try {
                $exports->{$function}($value);
                self::fail("Expected a TypeError for $function");
            } catch (\TypeError $error) {
                self::assertStringContainsString($message, $error->getMessage());
            }
        }
    }
}
