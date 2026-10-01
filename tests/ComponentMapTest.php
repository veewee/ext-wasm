<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Exception\LinkError;
use Wasm\Exception\RuntimeError;

/** WIT map<k, v> as a PHP array from key to value. */
final class ComponentMapTest extends TestCase
{
    private const COMPONENT = <<<'WAT'
        (component
          (core module $m
            (memory (export "memory") 1)
            (data (i32.const 300) "ab")
            (global $next (mut i32) (i32.const 1024))
            (func (export "realloc") (param i32 i32 i32 i32) (result i32)
              (local $p i32)
              (local.set $p (i32.and (i32.add (global.get $next) (i32.const 7)) (i32.const -8)))
              (global.set $next (i32.add (local.get $p) (local.get 3)))
              (local.get $p))
            ;; (ptr, len) of a list or map, returned unchanged
            (func (export "pair") (param i32 i32) (result i32)
              (i32.store (i32.const 0) (local.get 0))
              (i32.store (i32.const 4) (local.get 1))
              (i32.const 0))
            ;; record { counts: map<string, u32> }, returned unchanged
            (func (export "record") (param i32 i32) (result i32)
              (i32.store (i32.const 0) (local.get 0))
              (i32.store (i32.const 4) (local.get 1))
              (i32.const 0))
            ;; map<string, u32> with entries a: 1, b: 2, a: 3
            (func (export "dupes") (result i32)
              (i32.store (i32.const 400) (i32.const 300))
              (i32.store (i32.const 404) (i32.const 1))
              (i32.store (i32.const 408) (i32.const 1))
              (i32.store (i32.const 412) (i32.const 301))
              (i32.store (i32.const 416) (i32.const 1))
              (i32.store (i32.const 420) (i32.const 2))
              (i32.store (i32.const 424) (i32.const 300))
              (i32.store (i32.const 428) (i32.const 1))
              (i32.store (i32.const 432) (i32.const 3))
              (i32.store (i32.const 0) (i32.const 400))
              (i32.store (i32.const 4) (i32.const 3))
              (i32.const 0))
            ;; map<f32, u32> with one entry 1.0: 1
            (func (export "float-result") (result i32)
              (f32.store (i32.const 400) (f32.const 1))
              (i32.store (i32.const 404) (i32.const 1))
              (i32.store (i32.const 0) (i32.const 400))
              (i32.store (i32.const 4) (i32.const 1))
              (i32.const 0)))
          (core instance $i (instantiate $m))
          (alias core export $i "memory" (core memory $mem))
          (alias core export $i "realloc" (core func $realloc))

          (type $counts' (record (field "counts" (map string u32))))
          (export $counts "counts" (type $counts'))

          (func (export "strings") (param "m" (map string u32)) (result (map string u32))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "numbers") (param "m" (map u32 string)) (result (map u32 string))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "signed") (param "m" (map s64 bool)) (result (map s64 bool))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "wide") (param "m" (map u64 u8)) (result (map u64 u8))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "small") (param "m" (map u8 u8)) (result (map u8 u8))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "chars") (param "m" (map char u8)) (result (map char u8))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "flags") (param "m" (map bool string)) (result (map bool string))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "lists") (param "m" (list (map string u32))) (result (list (map string u32)))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "in-record") (param "r" $counts) (result $counts)
            (canon lift (core func $i "record") (memory $mem) (realloc $realloc)))
          (func (export "dupes") (result (map string u32))
            (canon lift (core func $i "dupes") (memory $mem) (realloc $realloc)))
          (func (export "floats") (param "m" (map f32 u32)) (result (map f32 u32))
            (canon lift (core func $i "pair") (memory $mem) (realloc $realloc)))
          (func (export "float-result") (result (map f32 u32))
            (canon lift (core func $i "float-result") (memory $mem) (realloc $realloc))))
        WAT;

    private static function exports(): Exports
    {
        return (new Instance(new Component(self::COMPONENT)))->exports;
    }

    public function test_a_map_is_an_array_from_key_to_value_in_order(): void
    {
        self::assertSame(['b' => 2, 'a' => 1], self::exports()->strings(['b' => 2, 'a' => 1]));
        self::assertSame([], self::exports()->strings([]));
    }

    /** @return iterable<string, array{string, array<array-key, mixed>}> */
    public static function keyTypes(): iterable
    {
        yield 'u32 keys' => ['numbers', [7 => 'seven', 0 => 'zero']];
        yield 's64 keys' => ['signed', [-5 => true, PHP_INT_MAX => false, PHP_INT_MIN => true]];
        yield 'u64 keys keep their bits' => ['wide', [-1 => 1, 5 => 2]];
        yield 'char keys' => ['chars', ['x' => 1, 'é' => 2, 7 => 3]];
        yield 'bool keys' => ['flags', [0 => 'no', 1 => 'yes']];
    }

    /** @param array<array-key, mixed> $map */
    #[DataProvider('keyTypes')]
    public function test_keys_of_every_supported_type_round_trip(string $function, array $map): void
    {
        self::assertSame($map, self::exports()->$function($map));
    }

    public function test_string_keys_follow_phps_array_key_rule(): void
    {
        $map = [
            '0' => 1, '-0' => 2, '+1' => 3, '01' => 4, ' 1' => 5, '1.0' => 6,
            '9223372036854775807' => 7, '9223372036854775808' => 8, '-9223372036854775808' => 9,
        ];

        $result = self::exports()->strings($map);

        self::assertSame($map, $result);
        self::assertSame(
            ['integer', 'string', 'string', 'string', 'string', 'string', 'integer', 'string', 'integer'],
            array_map(gettype(...), array_keys($result)),
        );
    }

    public function test_maps_nest_in_lists_and_records(): void
    {
        self::assertSame([['a' => 1], [], ['bc' => 2, 'd' => 3]], self::exports()->lists([['a' => 1], [], ['bc' => 2, 'd' => 3]]));
        self::assertSame(['counts' => ['x' => 9]], self::exports()->inRecord(['counts' => ['x' => 9]]));
    }

    public function test_a_duplicate_key_keeps_the_last_value_at_the_first_position(): void
    {
        self::assertSame(['a' => 3, 'b' => 2], self::exports()->dupes());
    }

    /** @return iterable<string, array{string, mixed, class-string<\Throwable>, string}> */
    public static function badMaps(): iterable
    {
        yield 'not an array' => ['strings', 'x', \TypeError::class, 'expected array'];
        yield 'a value of the wrong type' => ['strings', ['a' => 'x'], \TypeError::class, 'expected int'];
        yield 'a key of the wrong type' => ['numbers', ['x' => 'y'], \TypeError::class, 'u32'];
        yield 'a bool key other than 0 or 1' => ['flags', [2 => 'x'], \TypeError::class, 'bool'];
        yield 'a key out of range' => ['small', [300 => 1], \ValueError::class, 'out of range'];
        yield 'a char key of several characters' => ['chars', ['xy' => 1], \ValueError::class, 'char'];
        yield 'a key that is not UTF-8' => ['strings', ["\xff\xfe" => 1], \ValueError::class, 'map key'];
    }

    /** Keys get the errors values of their type get. */
    #[DataProvider('badMaps')]
    public function test_bad_maps_are_rejected_like_bad_values(string $function, mixed $value, string $class, string $message): void
    {
        $this->expectException($class);
        $this->expectExceptionMessage($message);

        self::exports()->$function($value);
    }

    public function test_a_map_with_keys_php_cannot_hold_fails_when_called(): void
    {
        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('map<f32, u32>');

        self::exports()->floats([]);
    }

    public function test_a_returned_map_with_keys_php_cannot_hold_fails_naming_its_type(): void
    {
        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('map<f32, u32>');

        self::exports()->floatResult();
    }

    public function test_a_bad_key_says_it_is_a_key(): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('map key "x": expected int for u32');

        self::exports()->numbers(['x' => 'y']);
    }

    public function test_a_php_import_receives_and_returns_maps(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (import "invert" (func $invert (param "m" (map string u32)) (result (map u32 string))))
              (core module $m
                (memory (export "memory") 1)
                (global $next (mut i32) (i32.const 1024))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                  (local $p i32)
                  (local.set $p (i32.and (i32.add (global.get $next) (i32.const 7)) (i32.const -8)))
                  (global.set $next (i32.add (local.get $p) (local.get 3)))
                  (local.get $p)))
              (core instance $libc (instantiate $m))
              (alias core export $libc "memory" (core memory $mem))
              (alias core export $libc "realloc" (core func $realloc))
              (core func $lowered (canon lower (func $invert) (memory $mem) (realloc $realloc)))
              (core module $caller
                (import "host" "invert" (func $invert (param i32 i32 i32)))
                (func (export "run") (param i32 i32) (result i32)
                  (call $invert (local.get 0) (local.get 1) (i32.const 16))
                  (i32.const 16)))
              (core instance $c (instantiate $caller (with "host" (instance (export "invert" (func $lowered))))))
              (func (export "run") (param "m" (map string u32)) (result (map u32 string))
                (canon lift (core func $c "run") (memory $mem) (realloc $realloc))))
            WAT);
        $received = null;
        $instance = new Instance($component, ['invert' => function (array $m) use (&$received): array {
            $received = $m;

            return array_map(strval(...), array_flip($m));
        }]);

        self::assertSame([1 => 'a', 2 => '7'], $instance->exports->run(['a' => 1, '7' => 2]));
        self::assertSame(['a' => 1, 7 => 2], $received);
    }

    public function test_the_link_check_looks_inside_map_values(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (type $point' (record (field "x" u32)))
              (import "point" (type $point (eq $point')))
              (import "take" (func (param "m" (map string (stream $point))))))
            WAT);

        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('stream<');

        new Instance($component, ['take' => static fn (array $m) => null]);
    }

    public function test_an_import_with_keys_php_cannot_hold_is_a_link_error(): void
    {
        $component = new Component('(component (import "take" (func (param "m" (map f32 u32)))))');

        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('map<f32, u32>');

        new Instance($component, ['take' => static fn (array $m) => null]);
    }

    public function test_maps_are_reflected(): void
    {
        $component = new Component(self::COMPONENT);
        $strings = array_column($component->exports(), null, 'name')['strings'];

        self::assertSame('func(m: map<string, u32>) -> map<string, u32>', $strings['type']);
        $map = $strings['signature']->params['m'];
        self::assertSame('map', $map->kind);
        self::assertSame('string', $map->key->kind);
        self::assertSame('u32', $map->element->kind);
    }

    public function test_keys_php_cannot_hold_are_still_reflected(): void
    {
        $floats = array_column((new Component(self::COMPONENT))->exports(), null, 'name')['floats'];

        self::assertSame('f32', $floats['signature']->params['m']->key->kind);
    }
}
