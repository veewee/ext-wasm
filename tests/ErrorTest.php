<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Wasm\Exception\CompileError;
use Wasm\Exception\LinkError;
use Wasm\Exception\RuntimeError;
use Wasm\Exception\WasmException;

final class ErrorTest extends TestCase
{
    /** @return iterable<array{class-string<WasmException>}> */
    public static function errors(): iterable
    {
        yield [WasmException::class];
        yield [CompileError::class];
        yield [LinkError::class];
        yield [RuntimeError::class];
    }

    /** @param class-string<WasmException> $class */
    #[DataProvider('errors')]
    public function test_errors_are_constructible_from_php(string $class): void
    {
        $error = new $class('went wrong', 7);

        self::assertInstanceOf(WasmException::class, $error);
        self::assertInstanceOf(\Exception::class, $error);
        self::assertSame('went wrong', $error->getMessage());
        self::assertSame(7, $error->getCode());
    }

    #[DataProvider('errors')]
    public function test_errors_have_defaults(string $class): void
    {
        $error = new $class();

        self::assertSame('', $error->getMessage());
        self::assertSame(0, $error->getCode());
    }

    /** @param class-string<WasmException> $class */
    #[DataProvider('errors')]
    public function test_the_constructor_belongs_to_the_error_class(string $class): void
    {
        self::assertSame($class, (new \ReflectionMethod($class, '__construct'))->getDeclaringClass()->getName());

        try {
            new $class([]);
            self::fail('Expected a TypeError');
        } catch (\TypeError $error) {
            self::assertStringStartsWith($class . '::__construct()', $error->getMessage());
        }
    }

    public function test_errors_can_be_thrown_and_caught(): void
    {
        $line = __LINE__ + 2;
        try {
            throw new LinkError('missing', 0, new \LogicException('cause'));
        } catch (WasmException $caught) {
            self::assertSame('missing', $caught->getMessage());
            self::assertSame(__FILE__, $caught->getFile());
            self::assertSame($line, $caught->getLine());
            self::assertInstanceOf(\LogicException::class, $caught->getPrevious());
        }
    }

    public function test_errors_thrown_by_the_extension_point_at_the_calling_php_code(): void
    {
        $line = __LINE__ + 2;
        try {
            new \Wasm\Module('(module nope');
        } catch (CompileError $caught) {
            self::assertSame(__FILE__, $caught->getFile());
            self::assertSame($line, $caught->getLine());
            self::assertNotEmpty($caught->getTrace());
        }
    }

    public function test_extension_errors_are_serializable_like_any_exception(): void
    {
        $error = new RuntimeError('trap');

        self::assertSame('trap', unserialize(serialize($error))->getMessage());
    }
}
