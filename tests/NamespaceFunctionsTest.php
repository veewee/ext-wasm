<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\CompileError;
use Wasm\Instance;
use Wasm\Module;

use function Wasm\compile;
use function Wasm\instantiate;
use function Wasm\validate;

final class NamespaceFunctionsTest extends TestCase
{
    private const WAT = '(module (import "env" "x" (global i32)) (func (export "x") (result i32) global.get 0))';

    public function test_validate(): void
    {
        self::assertTrue(validate("\0asm\x01\0\0\0"));
        self::assertTrue(validate('(module)'));
        self::assertFalse(validate("\0asm\x02\0\0\0"));
        self::assertFalse(validate('(module nope'));
    }

    public function test_compile(): void
    {
        self::assertInstanceOf(Module::class, compile('(module)'));

        $this->expectException(CompileError::class);
        compile('(module nope');
    }

    public function test_instantiate_from_bytes_returns_module_and_instance(): void
    {
        $result = instantiate(self::WAT, ['env' => ['x' => 5]]);

        self::assertSame(['module', 'instance'], array_keys($result));
        self::assertInstanceOf(Module::class, $result['module']);
        self::assertInstanceOf(Instance::class, $result['instance']);
        self::assertSame(5, $result['instance']->exports->x());
    }

    public function test_instantiate_from_module_returns_instance(): void
    {
        $instance = instantiate(new Module(self::WAT), ['env' => ['x' => 6]]);

        self::assertInstanceOf(Instance::class, $instance);
        self::assertSame(6, $instance->exports->x());
    }
}
