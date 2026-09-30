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

    public function test_a_store_with_sync_callbacks_cannot_take_suspending_imports(): void
    {
        $store = new Store();
        new Instance(new Module('(module (import "env" "f" (func)))'), ['env' => ['f' => fn () => null]], $store);

        $this->expectException(\Wasm\Exception\LinkError::class);
        $this->expectExceptionMessage('Suspending imports need a store without synchronous callbacks');
        new Instance(new Module(self::WAIT), ['env' => ['wait' => new Suspending(fn () => null)]], $store);
    }

    public function test_an_earlier_instance_keeps_working_after_its_store_turned_async(): void
    {
        $store = new Store();
        $first = (new Instance(new Module('(module (func (export "id") (param i32) (result i32) (local.get 0)))'), store: $store))->exports;
        new Instance(new Module(self::WAIT), ['env' => ['wait' => new Suspending(fn () => null)]], $store);

        self::assertSame(5, $first->id(5));
    }

    public function test_a_failed_instantiation_leaves_the_store_usable(): void
    {
        $store = new Store();
        try {
            new Instance(new Module('(module (import "env" "wait" (func)) (import "env" "gone" (func)))'), [
                'env' => ['wait' => new Suspending(fn () => null)],
            ], $store);
            self::fail('Expected a LinkError for the missing import');
        } catch (\Wasm\Exception\LinkError) {
        }

        $exports = (new Instance(new Module('(module (import "env" "f" (func (result i32))) (func (export "run") (result i32) (call 0)))'), [
            'env' => ['f' => fn (): int => 8],
        ], $store))->exports;

        self::assertSame(8, $exports->run());
    }

    public function test_wasi_start_runs_a_module_with_a_suspending_import(): void
    {
        $wasi = new \Wasm\Wasi();
        $module = new Module(<<<'EOWAT'
            (module
              (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
              (import "env" "later" (func $later (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 8) "hi\n")
              (func (export "_start")
                (i32.store (i32.const 0) (i32.const 8))
                (i32.store (i32.const 4) (call $later))
                (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 20)))))
            EOWAT);
        $imports = $wasi->getImportObject() + ['env' => ['later' => new Suspending(fn (): int => \Fiber::suspend())]];

        $fiber = new \Fiber(fn (): int => $wasi->start(new Instance($module, $imports)));
        $fiber->start();
        $fiber->resume(3);

        self::assertSame(0, $fiber->getReturn());
        self::assertSame("hi\n", $wasi->stdout());
    }

    public function test_wasi_initialize_runs_a_reactor_with_a_suspending_import(): void
    {
        $wasi = new \Wasm\Wasi();
        $module = new Module(<<<'EOWAT'
            (module
              (import "wasi_snapshot_preview1" "proc_exit" (func (param i32)))
              (import "env" "later" (func $later (result i32)))
              (memory (export "memory") 1)
              (global $ready (export "ready") (mut i32) (i32.const 0))
              (func (export "_initialize") (global.set $ready (call $later))))
            EOWAT);
        $instance = new Instance($module, $wasi->getImportObject() + [
            'env' => ['later' => new Suspending(fn (): int => \Fiber::suspend())],
        ]);

        $fiber = new \Fiber(fn () => $wasi->initialize($instance));
        $fiber->start();
        $fiber->resume(1);

        self::assertTrue($fiber->isTerminated());
        self::assertSame(1, $instance->exports->ready->value);
    }

    public function test_wasi_start_on_a_parked_store_is_busy(): void
    {
        $wasi = new \Wasm\Wasi();
        $module = new Module(<<<'EOWAT'
            (module
              (import "wasi_snapshot_preview1" "proc_exit" (func (param i32)))
              (import "env" "wait" (func $wait))
              (memory (export "memory") 1)
              (func (export "run") (call $wait))
              (func (export "_start")))
            EOWAT);
        $instance = new Instance($module, $wasi->getImportObject() + [
            'env' => ['wait' => new Suspending(fn () => \Fiber::suspend())],
        ]);
        $fiber = new \Fiber(fn () => $instance->exports->run());
        $fiber->start();

        try {
            $wasi->start($instance);
            self::fail('Expected the store to be busy');
        } catch (RuntimeError $busy) {
            self::assertSame('the store is busy with a suspended call', $busy->getMessage());
        }
        $fiber->resume();
    }
}
