<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Exception\CompileError;
use Wasm\Module;

final class ComponentTest extends TestCase
{
    private const ADDER = <<<'WAT'
        (component
          (core module $m
            (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))
          (core instance $i (instantiate $m))
          (func (export "add") (param "a" u32) (param "b" u32) (result u32)
            (canon lift (core func $i "add"))))
        WAT;

    private const LOGGER = <<<'WAT'
        (component
          (import "docs:demo/log@0.1.0" (instance
            (export "log" (func (param "line" string)))))
          (import "now" (func (result u64))))
        WAT;

    public function test_it_compiles_a_component_from_wat(): void
    {
        self::assertInstanceOf(Component::class, new Component(self::ADDER));
    }

    public function test_exports_list_functions_with_their_wit_signature(): void
    {
        self::assertSame(
            [['name' => 'add', 'kind' => 'function', 'type' => 'func(a: u32, b: u32) -> u32']],
            self::withoutSignatures((new Component(self::ADDER))->exports()),
        );
    }

    public function test_imports_list_interfaces_with_their_functions(): void
    {
        self::assertSame(
            [
                ['name' => 'docs:demo/log@0.1.0', 'kind' => 'instance', 'functions' => [
                    ['name' => 'log', 'kind' => 'function', 'type' => 'func(line: string)'],
                ]],
                ['name' => 'now', 'kind' => 'function', 'type' => 'func() -> u64'],
            ],
            self::withoutSignatures((new Component(self::LOGGER))->imports()),
        );
    }

    public function test_signatures_spell_out_compound_types(): void
    {
        // Records, enums, flags and variants must be named types in a signature.
        $component = new Component(<<<'WAT'
            (component
              (type $r' (record (field "first-name" string) (field "age" u8)))
              (import "person" (type $r (eq $r')))
              (type $e' (enum "red" "dark-blue"))
              (import "color" (type $e (eq $e')))
              (type $fl' (flags "read" "write"))
              (import "perms" (type $fl (eq $fl')))
              (type $v' (variant (case "none") (case "some" u32)))
              (import "maybe" (type $v (eq $v')))
              (import "f" (func
                (param "a" (list string))
                (param "b" (option (tuple u8 char)))
                (param "c" $r) (param "d" $e) (param "e" $fl) (param "f" $v)
                (result (result u32 (error string))))))
            WAT);

        self::assertSame(
            'func(a: list<string>, b: option<tuple<u8, char>>, c: record { first-name: string, age: u8 }, '
            . 'd: enum { red, dark-blue }, e: flags { read, write }, f: variant { none, some(u32) }) -> result<u32, string>',
            $component->imports()[0]['type'],
        );
    }

    public function test_from_file_compiles_a_component(): void
    {
        $file = tempnam(sys_get_temp_dir(), 'wasm-component');
        file_put_contents($file, self::ADDER);

        try {
            self::assertSame('add', Component::fromFile($file)->exports()[0]['name']);
        } finally {
            unlink($file);
        }
    }

    public function test_a_core_module_is_a_compile_error_with_a_hint(): void
    {
        $this->expectException(CompileError::class);
        $this->expectExceptionMessage('use Wasm\Module');
        new Component('(module)');
    }

    public function test_a_module_given_a_component_hints_at_component(): void
    {
        $this->expectException(CompileError::class);
        $this->expectExceptionMessage('use Wasm\Component\Component');
        new Module(self::ADDER);
    }

    public function test_invalid_bytes_are_a_compile_error(): void
    {
        $this->expectException(CompileError::class);
        new Component('not wasm');
    }

    public function test_validate_accepts_components(): void
    {
        self::assertTrue(\Wasm\validate(self::ADDER));
    }

    /**
     * The FunctionType objects of each entry, which ComponentTypeTest covers.
     *
     * @param list<array<string, mixed>> $entries
     * @return list<array<string, mixed>>
     */
    private static function withoutSignatures(array $entries): array
    {
        return array_map(function (array $entry): array {
            unset($entry['signature']);
            if (isset($entry['functions'])) {
                $entry['functions'] = self::withoutSignatures($entry['functions']);
            }

            return $entry;
        }, $entries);
    }
}
