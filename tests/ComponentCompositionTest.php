<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Component\Resource;
use Wasm\Exception\LinkError;
use Wasm\Wasi;

/**
 * composer.wasm imports the docs:demo/counters interface that counters.wasm
 * exports; the tests link one instance's exports into the other.
 */
final class ComponentCompositionTest extends TestCase
{
    private const FIXTURES = __DIR__ . '/fixtures/component-resources/';

    /** The counters interface of a fresh counters instance. */
    private static function counters(): Exports
    {
        return (new Instance(Component::fromFile(self::FIXTURES . 'counters.wasm'), wasi: new Wasi()))
            ->exports->get('docs:demo/counters');
    }

    private static function composer(mixed $counters): Exports
    {
        return (new Instance(Component::fromFile(self::FIXTURES . 'composer.wasm'), [
            'docs:demo/counters' => $counters,
        ], new Wasi()))->exports;
    }

    public function test_an_exported_interface_is_an_import_of_another_instance(): void
    {
        $counters = self::counters();

        self::assertSame(7, self::composer($counters)->makeAndCount(5));
    }

    public function test_a_resource_the_importer_drops_is_dropped_in_its_own_instance(): void
    {
        $counters = self::counters();

        self::composer($counters)->makeAndCount(5);

        self::assertSame(1, $counters->dropped());
    }

    public function test_a_handle_passed_through_the_importer_comes_back_as_itself(): void
    {
        $counters = self::counters();
        $counter = $counters->get('counter')->new(1);

        $returned = self::composer($counters)->passThrough($counter);

        self::assertInstanceOf(Resource::class, $returned);
        self::assertSame($counter, $returned);
        self::assertSame(2, $counter->value());
    }

    public function test_a_handle_lent_to_the_importer_stays_usable(): void
    {
        $counters = self::counters();
        $counter = $counters->get('counter')->new(3);
        $composer = self::composer($counters);

        self::assertSame(3, $composer->peek($counter));
        self::assertSame(4, $counter->increment());
    }

    public function test_the_importer_passes_borrowed_handles_on_to_the_exporter(): void
    {
        $counters = self::counters();
        $a = $counters->get('counter')->new(2);
        $b = $counters->get('counter')->new(5);

        self::assertSame(7, self::composer($counters)->totalOf($a, $b));
    }

    public function test_a_resource_class_can_implement_an_import_in_an_array(): void
    {
        $counters = self::counters();
        $composer = self::composer([
            'counter' => $counters->get('counter'),
            'total' => fn (Resource $a, Resource $b): int => $a->value() * 100 + $b->value(),
        ]);

        self::assertSame(7, $composer->makeAndCount(5));
        self::assertSame(102, $composer->totalOf($counters->get('counter')->new(1), $counters->get('counter')->new(2)));
    }

    public function test_a_dropped_handle_is_refused_on_the_way_into_the_importer(): void
    {
        $counters = self::counters();
        $counter = $counters->get('counter')->new(3);
        $counter->drop();
        $composer = self::composer($counters);

        try {
            $composer->peek($counter);
            self::fail('a dropped handle went into the importer');
        } catch (\Throwable $e) {
            self::assertStringContainsString('the resource was dropped', $e->getMessage());
        }
        // Refused before the call, so the importer is not poisoned.
        self::assertSame(4, $composer->peek($counters->get('counter')->new(4)));
    }

    public function test_a_handle_of_another_instance_is_refused(): void
    {
        $composer = self::composer(self::counters());
        $foreign = self::counters()->get('counter')->new(3);

        $this->expectExceptionMessage('expected a counter resource of the instance exporting it');
        $composer->peek($foreign);
    }

    public function test_an_interface_missing_a_function_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('total');
        self::composer(['counter' => self::counters()->get('counter')]);
    }
}
