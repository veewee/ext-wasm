<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exception\RuntimeError;
use Wasm\GlobalVar;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Store;
use Wasm\Suspending;

require_once __DIR__ . '/RunsPhpInSubprocess.php';

/**
 * While a Suspending call waits, its wasm frames are invisible to wasmtime's
 * GC, so the store refuses anything that could start one.
 */
final class SuspendingStoreTest extends TestCase
{
    use RunsPhpInSubprocess;

    private const WAIT = <<<'EOWAT'
        (module
          (import "env" "wait" (func $wait))
          (memory (export "memory") 1)
          (func (export "run") (call $wait))
          (func (export "id") (param i32) (result i32) (local.get 0)))
        EOWAT;

    public function test_another_fiber_cannot_call_into_a_parked_store(): void
    {
        $exports = (new Instance(new Module(self::WAIT), [
            'env' => ['wait' => new Suspending(fn () => \Fiber::suspend())],
        ]))->exports;
        $fiber = new \Fiber(fn () => $exports->run());
        $fiber->start();

        try {
            $exports->id(1);
            self::fail('Expected the store to be busy');
        } catch (RuntimeError $busy) {
            self::assertSame('the store is busy with a suspended call', $busy->getMessage());
        }

        $fiber->resume();
        self::assertSame(2, $exports->id(2));
    }

    public function test_a_callback_cannot_reenter_a_parked_store(): void
    {
        $exports = null;
        $exports = (new Instance(new Module(self::WAIT), [
            'env' => ['wait' => new Suspending(function () use (&$exports): void {
                $exports->id(1);
            })],
        ]))->exports;

        try {
            $exports->run();
            self::fail('Expected the store to be busy');
        } catch (RuntimeError $busy) {
            self::assertSame('the store is busy with a suspended call', $busy->getMessage());
        }
    }

    public function test_memory_of_a_parked_store_is_usable_from_another_fiber(): void
    {
        $instance = new Instance(new Module(self::WAIT), [
            'env' => ['wait' => new Suspending(fn () => \Fiber::suspend())],
        ]);
        $fiber = new \Fiber(fn () => $instance->exports->run());
        $fiber->start();

        $instance->exports->memory->write(0, 'ok');
        self::assertSame('ok', $instance->exports->memory->read(0, 2));

        $fiber->resume();
        self::assertTrue($fiber->isTerminated());
    }

    public function test_instantiating_into_a_parked_store_is_busy(): void
    {
        $store = new Store();
        $exports = (new Instance(new Module(self::WAIT), [
            'env' => ['wait' => new Suspending(fn () => \Fiber::suspend())],
        ], $store))->exports;
        $fiber = new \Fiber(fn () => $exports->run());
        $fiber->start();

        try {
            new Instance(new Module('(module)'), store: $store);
            self::fail('Expected the store to be busy');
        } catch (RuntimeError $busy) {
            self::assertSame('the store is busy with a suspended call', $busy->getMessage());
        }
        $fiber->resume();
    }

    public function test_creating_an_externref_in_a_parked_store_is_busy(): void
    {
        $store = new Store();
        $global = new GlobalVar(['value' => 'externref', 'mutable' => true], null, $store);
        $exports = (new Instance(new Module(self::WAIT), [
            'env' => ['wait' => new Suspending(fn () => \Fiber::suspend())],
        ], $store))->exports;
        $fiber = new \Fiber(fn () => $exports->run());
        $fiber->start();

        try {
            $global->value = new \stdClass();
            self::fail('Expected the store to be busy');
        } catch (RuntimeError $busy) {
            self::assertSame('the store is busy with a suspended call', $busy->getMessage());
        }
        $fiber->resume();
        $global->value = $object = new \stdClass();
        self::assertSame($object, $global->value);
    }

    public function test_an_externref_held_across_a_suspension_survives_later_allocations(): void
    {
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "wait" (func $wait))
              (import "env" "make" (func $make (result externref)))
              (func (export "keep") (param $x externref) (result externref)
                (local $i i32)
                (call $wait)
                (loop $again
                  (drop (call $make))
                  (local.set $i (i32.add (local.get $i) (i32.const 1)))
                  (br_if $again (i32.lt_u (local.get $i) (i32.const 20000))))
                (local.get $x)))
            EOWAT), ['env' => [
            'wait' => new Suspending(fn () => \Fiber::suspend()),
            'make' => fn () => new \stdClass(),
        ]]))->exports;
        $kept = new \stdClass();

        $fiber = new \Fiber(fn () => $exports->keep($kept));
        $fiber->start();
        $fiber->resume();

        self::assertSame($kept, $fiber->getReturn());
    }
}
