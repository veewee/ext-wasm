<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\GlobalVar;
use Wasm\Instance;
use Wasm\Module;

final class GlobalTest extends TestCase
{
    private const WAT = <<<'EOWAT'
        (module
          (global $one (export "one") i32 (i32.const 1))
          (global $some (export "some") (mut i32) (i32.const 0))
          (func (export "get_some") (result i32) (global.get $some))
          (func (export "set_some") (param i32) (global.set $some (local.get 0))))
        EOWAT;

    public function test_it_reads_and_writes_exported_globals(): void
    {
        $exports = (new Instance(new Module(self::WAT)))->exports;

        self::assertInstanceOf(GlobalVar::class, $exports->some);
        self::assertSame(0, $exports->some->value);

        $exports->some->value = 1;
        self::assertSame(1, $exports->some->value);
        self::assertSame(1, $exports->get_some());

        $exports->set_some(21);
        self::assertSame(21, $exports->some->value);
        self::assertSame(21, $exports->some->valueOf());
    }

    public function test_it_can_not_change_immutable_globals(): void
    {
        $exports = (new Instance(new Module(self::WAT)))->exports;

        self::assertSame(1, $exports->one->value);

        $this->expectException(\TypeError::class);
        $exports->one->value = 2;
    }

    public function test_it_creates_standalone_globals(): void
    {
        $i64 = new GlobalVar(['value' => 'i64', 'mutable' => true], PHP_INT_MAX);
        $f64 = new GlobalVar(['value' => 'f64'], 1.5);
        $default = new GlobalVar(['value' => 'i32']);

        self::assertSame(PHP_INT_MAX, $i64->value);
        self::assertSame(1.5, $f64->value);
        self::assertSame(0, $default->value);

        $i64->value = -1;
        self::assertSame(-1, $i64->value);
    }

    public function test_it_validates_the_initial_value(): void
    {
        $this->expectException(\TypeError::class);
        new GlobalVar(['value' => 'i32'], 1.1);
    }

    public function test_it_rejects_unknown_value_types(): void
    {
        $this->expectException(\TypeError::class);
        new GlobalVar(['value' => 'i8']);
    }

    public function test_it_requires_a_value_type(): void
    {
        $this->expectException(\TypeError::class);
        new GlobalVar(['mutable' => true]);
    }
}
