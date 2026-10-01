<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Exception\LinkError;
use Wasm\Wasi;

/** Implements the `logger` resource that logger.wasm imports. */
final class PrefixLogger
{
    /** @var list<string> */
    public static array $destroyed = [];

    /** @var (\Closure(): void)|null */
    public static ?\Closure $onDestruct = null;

    public function __construct(private string $prefix)
    {
    }

    public function write(string $line): string
    {
        return "[$this->prefix] $line";
    }

    public function __destruct()
    {
        self::$destroyed[] = $this->prefix;
        if (self::$onDestruct !== null) {
            (self::$onDestruct)();
        }
    }
}

abstract class AbstractLogger
{
    abstract public function write(string $line): string;
}

final class LoggerWithoutWrite
{
    public function __construct(string $prefix)
    {
    }
}

/**
 * tests/fixtures/component-resources/logger.wasm imports a `logger`
 * resource and uses it in three exports; PHP classes implement it.
 */
final class ComponentHostResourceTest extends TestCase
{
    private const LOGGER = __DIR__ . '/fixtures/component-resources/logger.wasm';

    protected function setUp(): void
    {
        PrefixLogger::$destroyed = [];
        PrefixLogger::$onDestruct = null;
    }

    private static function exports(string $class = PrefixLogger::class): Exports
    {
        return (new Instance(Component::fromFile(self::LOGGER), [
            'docs:demo/log' => ['logger' => $class],
        ], new Wasi()))->exports;
    }

    public function test_the_component_constructs_and_calls_a_php_object(): void
    {
        self::assertSame('[app] hello', self::exports()->usesLogger('app', 'hello'));
    }

    public function test_the_component_dropping_its_object_releases_it(): void
    {
        self::exports()->usesLogger('app', 'hello');

        self::assertSame(['app'], PrefixLogger::$destroyed);
    }

    public function test_an_object_given_and_returned_comes_back_as_itself(): void
    {
        $logger = new PrefixLogger('mine');

        self::assertSame($logger, self::exports()->echoLogger($logger));
        self::assertSame([], PrefixLogger::$destroyed);
    }

    public function test_an_object_lent_to_the_component_stays_with_php(): void
    {
        $logger = new PrefixLogger('lent');
        $exports = self::exports();

        self::assertSame('[lent] one', $exports->borrowLogger($logger, 'one'));
        self::assertSame('[lent] two', $exports->borrowLogger($logger, 'two'));
        self::assertSame([], PrefixLogger::$destroyed);

        // The component keeps no reference once the call is over.
        unset($logger);
        self::assertSame(['lent'], PrefixLogger::$destroyed);
    }

    public function test_an_object_of_another_class_is_a_type_error(): void
    {
        $this->expectException(\TypeError::class);
        self::exports()->borrowLogger(new \stdClass(), 'x');
    }

    public function test_a_destructor_run_by_the_component_can_use_wasm(): void
    {
        $exports = self::exports();
        $seen = null;
        PrefixLogger::$onDestruct = function () use ($exports, &$seen): void {
            PrefixLogger::$onDestruct = null;
            $seen = $exports->borrowLogger(new PrefixLogger('inner'), 'from a destructor');
        };

        $exports->usesLogger('app', 'hello');

        self::assertSame('[inner] from a destructor', $seen);
    }

    public function test_a_class_without_a_method_the_resource_declares_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('write');
        self::exports(LoggerWithoutWrite::class);
    }

    public function test_a_resource_must_be_a_class_name(): void
    {
        $this->expectException(LinkError::class);
        self::exports('NoSuchClass');
    }

    public function test_objects_lent_during_a_nested_call_all_come_back(): void
    {
        NestingLogger::$destroyed = [];
        $exports = self::exports(NestingLogger::class);
        // The outer object's write() makes a nested call that lends another object.
        $outer = new NestingLogger('outer', $exports);

        self::assertSame('[outer] hi', $exports->borrowLogger($outer, 'hi'));
        unset($outer);

        self::assertSame(['inner', 'outer'], NestingLogger::$destroyed);
    }

    public function test_an_object_given_before_a_failing_argument_is_not_kept(): void
    {
        $exports = self::exports();

        try {
            $exports->takeLogger(new PrefixLogger('kept?'), 'not a number');
            self::fail('Expected a TypeError');
        } catch (\TypeError) {
        }

        self::assertSame(['kept?'], PrefixLogger::$destroyed);
    }

    public function test_an_abstract_class_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('instantiable');
        self::exports(AbstractLogger::class);
    }
}

/** A logger whose write() calls the component again with another logger. */
final class NestingLogger
{
    /** @var list<string> */
    public static array $destroyed = [];

    public function __construct(private string $prefix, private ?Exports $exports = null)
    {
    }

    public function write(string $line): string
    {
        if ($this->exports !== null) {
            $this->exports->borrowLogger(new NestingLogger('inner'), 'x');
        }

        return "[$this->prefix] $line";
    }

    public function __destruct()
    {
        self::$destroyed[] = $this->prefix;
    }
}
