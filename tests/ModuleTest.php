<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\CompileError;
use Wasm\Exception\WasmException;
use Wasm\Module;

final class ModuleTest extends TestCase
{
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
}
