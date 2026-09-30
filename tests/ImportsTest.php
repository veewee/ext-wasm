<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\LinkError;
use Wasm\GlobalVar;
use Wasm\Instance;
use Wasm\Memory;
use Wasm\Module;

final class ImportsTest extends TestCase
{
    private const GLOBAL_WAT = <<<'EOWAT'
        (module
          (import "env" "global" (global $global (mut i32)))
          (func (export "read_g") (result i32) global.get $global)
          (func (export "write_g") (param i32) local.get 0 global.set $global))
        EOWAT;

    public function test_it_fails_on_missing_imports(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessageMatches('/env.*global/');
        new Instance(new Module(self::GLOBAL_WAT));
    }

    public function test_it_fails_on_unknown_module(): void
    {
        $this->expectException(LinkError::class);
        new Instance(new Module(self::GLOBAL_WAT), ['unknown' => ['global' => $this->mutableGlobal(32)]]);
    }

    public function test_it_fails_on_unknown_import_key(): void
    {
        $this->expectException(LinkError::class);
        new Instance(new Module(self::GLOBAL_WAT), ['env' => ['unknown' => $this->mutableGlobal(32)]]);
    }

    public function test_it_fails_on_invalid_type(): void
    {
        $this->expectException(LinkError::class);
        new Instance(new Module(self::GLOBAL_WAT), [
            'env' => ['global' => new GlobalVar(['value' => 'i64', 'mutable' => true], 32)],
        ]);
    }

    public function test_it_fails_on_invalid_mutability(): void
    {
        $this->expectException(LinkError::class);
        new Instance(new Module(self::GLOBAL_WAT), [
            'env' => ['global' => new GlobalVar(['value' => 'i32'], 32)],
        ]);
    }

    public function test_it_fails_on_wrong_kind(): void
    {
        $this->expectException(LinkError::class);
        new Instance(new Module(self::GLOBAL_WAT), ['env' => ['global' => new Memory(['initial' => 1])]]);
    }

    public function test_it_imports_globals_by_reference(): void
    {
        $global = $this->mutableGlobal(32);
        $exports = (new Instance(new Module(self::GLOBAL_WAT), ['env' => ['global' => $global]]))->exports;

        self::assertSame(32, $exports->read_g());

        $exports->write_g(5);
        self::assertSame(5, $global->value);

        $global->value = 7;
        self::assertSame(7, $exports->read_g());
    }

    public function test_it_accepts_numbers_for_immutable_global_imports(): void
    {
        $module = new Module(<<<'EOWAT'
            (module
              (import "env" "g" (global $g f64))
              (func (export "read") (result f64) global.get $g))
            EOWAT);

        self::assertSame(2.5, (new Instance($module, ['env' => ['g' => 2.5]]))->exports->read());
    }

    public function test_one_memory_is_shared_between_instances(): void
    {
        $module = new Module(<<<'EOWAT'
            (module
              (import "env" "memory" (memory 1))
              (func (export "store") (param i32 i32) (i32.store8 (local.get 0) (local.get 1)))
              (func (export "load") (param i32) (result i32) (i32.load8_u (local.get 0))))
            EOWAT);
        $memory = new Memory(['initial' => 1]);

        $writer = new Instance($module, ['env' => ['memory' => $memory]]);
        $reader = new Instance($module, ['env' => ['memory' => $memory]]);

        $writer->exports->store(3, 99);
        self::assertSame(99, $reader->exports->load(3));
        self::assertSame(chr(99), $memory->read(3, 1));
    }

    public function test_exports_of_one_instance_can_be_imports_of_another(): void
    {
        $producer = new Instance(new Module('(module (global (export "g") (mut i32) (i32.const 11)))'));
        $consumer = new Instance(new Module(self::GLOBAL_WAT), ['env' => ['global' => $producer->exports->g]]);

        self::assertSame(11, $consumer->exports->read_g());
    }

    private function mutableGlobal(int $value): GlobalVar
    {
        return new GlobalVar(['value' => 'i32', 'mutable' => true], $value);
    }
}
