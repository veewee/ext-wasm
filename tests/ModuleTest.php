<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\CompileError;
use Wasm\Exception\WasmException;
use Wasm\Module;

final class ModuleTest extends TestCase
{
    use RunsPhpInSubprocess;

    public function test_it_compiles_wat(): void
    {
        self::assertInstanceOf(Module::class, new Module('(module)'));
    }

    public function test_it_compiles_binary(): void
    {
        // Smallest valid module: magic + version.
        self::assertInstanceOf(Module::class, new Module("\0asm\x01\0\0\0"));
    }

    public function test_it_throws_compile_error_on_invalid_wat(): void
    {
        $this->expectException(CompileError::class);
        new Module('(module INVALIDWAT');
    }

    public function test_it_throws_compile_error_on_invalid_binary(): void
    {
        $this->expectException(CompileError::class);
        new Module("\0asm\x02\0\0\0");
    }

    public function test_compile_error_is_a_wasm_exception(): void
    {
        self::assertTrue(is_subclass_of(CompileError::class, WasmException::class));
        self::assertTrue(is_subclass_of(WasmException::class, \Exception::class));
    }

    public function test_from_file_compiles_a_binary_or_wat_file(): void
    {
        $dir = WasiTest::tempDir();
        file_put_contents("$dir/add.wat", '(module (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))');
        file_put_contents("$dir/empty.wasm", "\0asm\x01\0\0\0");

        $module = Module::fromFile("$dir/add.wat");
        self::assertSame(5, (new \Wasm\Instance($module))->exports->add(2, 3));
        self::assertSame([], Module::fromFile("$dir/empty.wasm")->exports());
    }

    public function test_from_file_resolves_relative_paths_against_the_php_working_directory(): void
    {
        $dir = WasiTest::tempDir();
        file_put_contents("$dir/relative.wat", '(module)');
        $previous = getcwd();
        chdir($dir);
        try {
            self::assertInstanceOf(Module::class, Module::fromFile('relative.wat'));
        } finally {
            chdir($previous);
        }
    }

    public function test_from_file_throws_on_a_missing_file(): void
    {
        $this->expectException(WasmException::class);
        $this->expectExceptionMessageMatches('/cannot read .*missing\.wasm/');
        Module::fromFile(sys_get_temp_dir() . '/missing.wasm');
    }

    public function test_from_file_throws_compile_error_on_invalid_contents(): void
    {
        $dir = WasiTest::tempDir();
        file_put_contents("$dir/broken.wasm", 'not wasm');

        $this->expectException(CompileError::class);
        Module::fromFile("$dir/broken.wasm");
    }

    public function test_from_file_respects_open_basedir(): void
    {
        $dir = WasiTest::tempDir();
        mkdir("$dir/allowed");
        file_put_contents("$dir/outside.wat", '(module)');
        file_put_contents("$dir/allowed/inside.wat", '(module)');

        $code = <<<'PHP'
            <?php
            Wasm\Module::fromFile('DIR/allowed/inside.wat');
            echo "inside ok\n";
            try {
                Wasm\Module::fromFile('DIR/outside.wat');
                echo 'outside read';
            } catch (Wasm\Exception\WasmException $e) {
                echo get_class($e), ': ', $e->getMessage();
            }
            PHP;
        $output = $this->runPhp(str_replace('DIR', $dir, $code), settings: ['open_basedir' => "$dir/allowed"]);

        self::assertStringContainsString('inside ok', $output);
        self::assertMatchesRegularExpression('/WasmException: cannot read .*outside\.wat.*open_basedir/', $output);
    }
}
