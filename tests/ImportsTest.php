<?php

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\InstanceBuilder;
use Wasm\Imports;
use Wasm\Type;

class ImportsTest extends TestCase
{
    public function test_it_fails_on_missing_imports() {
        $this->expectException(\Exception::class);
        $this->createBuilderFromWat()->build();
    }

    public function test_it_fails_on_unkown_env() {
        $imports = Imports::create();
        $imports->define('unkown', 'global', Type\Global::mutable(32));

        $builder = $this->createBuilderFromWat();
        $builder->import($imports);

        $this->expectException(\Exception::class);
        $builder->build();
    }

    public function test_it_fails_on_unkown_import_key() {
        $imports = Imports::create();
        $imports->define('env', 'unkown', Type\Global::mutable(32));

        $builder = $this->createBuilderFromWat();
        $builder->import($imports);

        $this->expectException(\Exception::class);
        $builder->build();
    }

    public function test_it_fails_on_invalid_type() {
        $imports = Imports::create();
        $imports->define('env', 'global', Type\Global::mutable(1.1));

        $builder = $this->createBuilderFromWat();
        $builder->import($imports);

        $this->expectException(\Exception::class);
        $builder->build();
    }

    public function test_it_fails_on_invalid_mutability() {
        $imports = Imports::create();
        $imports->define('env', 'global', Type\Global::immutable(32));

        $builder = $this->createBuilderFromWat();
        $builder->import($imports);

        $this->expectException(\Exception::class);
        $builder->build();
    }

    public function test_it_imports_simple_globals() {
        $imports = Imports::create();
        $imports->define('env', 'global', Type\Global::mutable(32));

        $builder = $this->createBuilderFromWat();
        $builder->import($imports);

        $instance = $builder->build();

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
