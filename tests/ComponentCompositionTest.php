<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;
use Wasm\Component\Resource;
use Wasm\Exception\LinkError;
use Wasm\Suspending;
use Wasm\Wasi;

/**
 * composer.wasm imports the docs:demo/counters interface that counters.wasm
 * exports; the tests link one instance's exports into the other.
 */
final class ComponentCompositionTest extends TestCase
{
    private const FIXTURES = __DIR__ . '/fixtures/component-resources/';

    /** Exports run(n), which returns later(n) + 1. */
    private const SUSPENDS = <<<'WAT'
        (component
          (import "later" (func $later (param "n" u32) (result u32)))
          (core func $later-core (canon lower (func $later)))
          (core module $m
            (import "host" "later" (func $later (param i32) (result i32)))
            (func (export "run") (param i32) (result i32) (i32.add (call $later (local.get 0)) (i32.const 1))))
          (core instance $i (instantiate $m (with "host" (instance (export "later" (func $later-core))))))
          (func (export "run") (param "n" u32) (result u32) (canon lift (core func $i "run"))))
        WAT;

    /** Exports a resource thing with only a constructor. */
    private const THING = <<<'WAT'
        (component
          (type $thing' (resource (rep i32)))
          (core func $new (canon resource.new $thing'))
          (core module $m
            (import "host" "new" (func $new (param i32) (result i32)))
            (func (export "ctor") (param i32) (result i32) (call $new (local.get 0))))
          (core instance $i (instantiate $m (with "host" (instance (export "new" (func $new))))))
          (export $thing "thing" (type $thing'))
          (func (export "[constructor]thing") (param "start" u32) (result (own $thing)) (canon lift (core func $i "ctor"))))
        WAT;

    private static function suspends(): Exports
    {
        return (new Instance(new Component(self::SUSPENDS), [
            'later' => new Suspending(fn (int $n): int => \Fiber::suspend() + $n),
        ]))->exports;
    }

    /** The counters interface of a fresh counters instance. */
    private static function counters(): Exports
    {
        return (new Instance(Component::fromFile(self::FIXTURES . 'counters.wasm'), wasi: new Wasi()))
            ->exports->get('docs:demo/counters');
    }

    /**
     * An importer whose `go(n)` constructs a counter with `n`, declared as s32
     * where the exporter takes u32; with `$later` it first passes `n` through
     * a Suspending import.
     */
    private static function importer(Exports $counters, ?Suspending $later = null): Exports
    {
        $laterImport = $later ? '(import "later" (func $later (param "n" s32) (result s32)))
            (core func $later-core (canon lower (func $later)))' : '';
        $laterCoreImport = $later ? '(import "host" "later" (func $later (param i32) (result i32)))' : '';
        $laterCall = $later ? '(local.set 0 (call $later (local.get 0)))' : '';
        $laterExport = $later ? '(export "later" (func $later-core))' : '';
        $wat = <<<WAT
            (component
              (import "docs:demo/counters@0.1.0" (instance \$cs
                (export "counter" (type \$counter (sub resource)))
                (export "[constructor]counter" (func (param "start" s32) (result (own \$counter))))))
              (alias export \$cs "[constructor]counter" (func \$new))
              (core func \$new-core (canon lower (func \$new)))
              {$laterImport}
              (core module \$m
                (import "host" "new" (func \$new (param i32) (result i32)))
                {$laterCoreImport}
                (func (export "go") (param i32) (result i32)
                  {$laterCall}
                  (drop (call \$new (local.get 0)))
                  (i32.const 7)))
              (core instance \$i (instantiate \$m (with "host" (instance (export "new" (func \$new-core)) {$laterExport}))))
              (func (export "go") (param "n" s32) (result u32) (canon lift (core func \$i "go"))))
            WAT;
        $imports = ['docs:demo/counters' => $counters];
        if ($later) {
            $imports['later'] = $later;
        }

        return (new Instance(new Component($wat), $imports))->exports;
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

    public function test_an_error_of_the_exporter_reaches_php_with_its_message(): void
    {
        $importer = self::importer(self::counters());

        self::assertSame(7, $importer->go(1));
        $this->expectException(\ValueError::class);
        $this->expectExceptionMessage('-1 is out of range for u32');
        $importer->go(-1);
    }

    public function test_an_async_importer_calls_the_exporter_after_resuming(): void
    {
        $counters = self::counters();
        $importer = self::importer($counters, new Suspending(fn (int $n): int => \Fiber::suspend() + $n));

        $fiber = new \Fiber(fn (): int => $importer->go(1));
        $fiber->start();
        $fiber->resume(2);
        self::assertSame(7, $fiber->getReturn());

        $failing = new \Fiber(fn (): int => $importer->go(1));
        $failing->start();
        $this->expectExceptionMessage('-2 is out of range for u32');
        $failing->resume(-3);
    }

    public function test_exports_without_the_resource_are_a_link_error_naming_it(): void
    {
        $instance = new Instance(Component::fromFile(self::FIXTURES . 'counters.wasm'), wasi: new Wasi());

        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('the exported interface given for "docs:demo/counters@0.1.0" has no resource "counter"');
        self::composer($instance->exports);
    }

    public function test_a_call_through_a_suspending_import_may_suspend_in_the_exporter(): void
    {
        $counters = self::counters();
        $exporter = self::suspends();
        $composer = self::composer([
            'counter' => $counters->get('counter'),
            'total' => new Suspending(fn (Resource $a, Resource $b): int => $exporter->run($a->value())),
        ]);

        $fiber = new \Fiber(fn (): int => $composer->totalOf($counters->get('counter')->new(1), $counters->get('counter')->new(2)));
        $fiber->start();
        $fiber->resume(5);

        self::assertSame(7, $fiber->getReturn());
    }

    public function test_a_call_through_a_plain_import_cannot_suspend_in_the_exporter(): void
    {
        $counters = self::counters();
        $exporter = self::suspends();
        $composer = self::composer([
            'counter' => $counters->get('counter'),
            'total' => fn (Resource $a, Resource $b): int => $exporter->run($a->value()),
        ]);

        $this->expectException(\FiberError::class);
        (new \Fiber(fn (): int => $composer->totalOf($counters->get('counter')->new(1), $counters->get('counter')->new(2))))->start();
    }

    public function test_a_resource_class_lacking_an_imported_method_is_a_link_error(): void
    {
        $thing = (new Instance(new Component(self::THING)))->exports->get('thing');

        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('the resource "thing" given for "docs:demo/counters@0.1.0#[method]counter.');
        self::composer(['counter' => $thing, 'total' => fn (): int => 0]);
    }

    public function test_an_interface_missing_a_function_is_a_link_error(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('total');
        self::composer(['counter' => self::counters()->get('counter')]);
    }
}
