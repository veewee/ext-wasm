<?php

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\InstanceBuilder;

class ImportsTest extends TestCase
{
    public function test_it_fails_on_missing_imports() {
        $this->expectException(\Exception::class);
        $this->createBuilderFromWat()->build();
    }

    public function test_it_fails_on_unkown_env() {
        $this->expectException(\Exception::class);
        $this->createBuilderFromWat()->withImports(
            \Wasm\Imports::create()
                ->define('unkown', 'global', \Wasm\Type\Global::mutable(32))
        )->build();
    }

    public function test_it_fails_on_unkown_import_key() {
        $this->expectException(\Exception::class);
        $this->createBuilderFromWat()->withImports(
            \Wasm\Imports::create()
                ->define('env', 'unkown', \Wasm\Type\Global::mutable(32))
        )->build();
    }

    public function test_it_fails_on_invalid_type() {
        $this->expectException(\Exception::class);
        $this->createBuilderFromWat()->withImports(
            \Wasm\Imports::create()
                ->define('env', 'global', \Wasm\Type\Global::mutable(1.1))
        )->build();
    }

    public function test_it_fails_on_invalid_mutability() {
        $this->expectException(\Exception::class);
        $this->createBuilderFromWat()->withImports(
            \Wasm\Imports::create()
                ->define('env', 'global', \Wasm\Type\Global::immutable(32))
        )->build();
    }

    public function test_it_imports_simple_globals() {
        $instance = $this->createBuilderFromWat()->withImports(
            \Wasm\Imports::create()
                ->define('env', 'global', \Wasm\Type\Global::mutable(32))
        )->build();

        self::assertSame(32, $instance->read_g());
    }

    private function createBuilderFromWat(?string $wat = null): InstanceBuilder
    {
        return InstanceBuilder::fromWat($wat ?? <<<'EOWAT'
            (module
              (import "env" "global" (global $global (mut i32)))
              (func (export "read_g") (result i32)
                global.get $global)
              (func (export "write_g") (param i32)
                local.get 0
                global.set $global))
            EOWAT);
    }
}
