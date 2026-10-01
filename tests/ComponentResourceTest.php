<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Component\Resource;
use Wasm\Component\ResourceClass;
use Wasm\Wasi;

/**
 * tests/fixtures/component-resources/counters.wasm exports a `counter`
 * resource defined in Rust; dropped() counts how often its destructor ran.
 */
final class ComponentResourceTest extends TestCase
{
    private const COUNTERS = __DIR__ . '/fixtures/component-resources/counters.wasm';

    private static ?Component $component = null;

    private static function counters(): Exports
    {
        self::$component ??= Component::fromFile(self::COUNTERS);

        return (new Instance(self::$component, wasi: new Wasi()))->exports->get('docs:demo/counters');
    }

    public function test_a_resource_type_is_a_resource_class(): void
    {
        self::assertInstanceOf(ResourceClass::class, self::counters()->get('counter'));
    }

    public function test_the_constructor_and_methods_call_the_component(): void
    {
        $counter = self::counters()->get('counter')->new(5);

        self::assertInstanceOf(Resource::class, $counter);
        self::assertSame(6, $counter->increment());
        self::assertSame(7, $counter->increment());
        self::assertSame(7, $counter->value());
    }

    public function test_a_static_function_is_a_method_of_the_class(): void
    {
        self::assertSame(0, self::counters()->get('counter')->zero()->value());
    }

    public function test_resource_functions_are_not_listed_as_functions_of_the_interface(): void
    {
        $names = array_keys(iterator_to_array(self::counters()));

        self::assertSame(['counter', 'total', 'consume', 'dropped'], $names);
    }

    public function test_drop_runs_the_component_destructor(): void
    {
        $counters = self::counters();
        $counter = $counters->get('counter')->new(1);

        $counter->drop();

        self::assertSame(1, $counters->dropped());
    }

    public function test_the_php_destructor_drops_the_handle(): void
    {
        $counters = self::counters();
        $counter = $counters->get('counter')->new(1);

        unset($counter);

        self::assertSame(1, $counters->dropped());
    }

    public function test_a_dropped_resource_cannot_be_used(): void
    {
        $counter = self::counters()->get('counter')->new(1);
        $counter->drop();

        $this->expectException(\Error::class);
        $this->expectExceptionMessage('dropped');
        $counter->value();
    }

    public function test_dropping_twice_is_harmless(): void
    {
        $counters = self::counters();
        $counter = $counters->get('counter')->new(1);

        $counter->drop();
        $counter->drop();

        self::assertSame(1, $counters->dropped());
    }

    public function test_a_borrow_parameter_leaves_the_resource_with_php(): void
    {
        $counters = self::counters();
        $a = $counters->get('counter')->new(2);
        $b = $counters->get('counter')->new(3);

        self::assertSame(5, $counters->total($a, $b));
        self::assertSame(3, $a->increment());
    }

    public function test_an_own_parameter_moves_the_resource_into_the_component(): void
    {
        $counters = self::counters();
        $counter = $counters->get('counter')->new(4);

        self::assertSame(4, $counters->consume($counter));
        self::assertSame(1, $counters->dropped());

        $this->expectException(\Error::class);
        $this->expectExceptionMessage('moved');
        $counter->value();
    }

    public function test_a_failed_conversion_does_not_move_the_resource(): void
    {
        $counters = self::counters();
        $counter = $counters->get('counter')->new(4);

        try {
            $counters->total($counter, 'not a counter');
            self::fail('Expected a TypeError');
        } catch (\TypeError) {
        }

        self::assertSame(4, $counter->value());
    }

    public function test_a_resource_of_another_instance_is_a_type_error(): void
    {
        $counter = self::counters()->get('counter')->new(1);

        $this->expectException(\TypeError::class);
        self::counters()->total($counter, $counter);
    }

    public function test_a_resource_cannot_be_cloned(): void
    {
        $counter = self::counters()->get('counter')->new(1);

        $this->expectException(\Error::class);
        $copy = clone $counter;
    }
}
