<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Instance;

/**
 * The async component model: `async func` exports and imports, streams and
 * futures, on wasmtime's concurrent event loop.
 */
final class ComponentAsyncTest extends TestCase
{
    /** An async export run() that returns plain() + 1, through task.return. */
    private const ASYNC_EXPORT = <<<'WAT'
        (component
          (import "plain" (func $plain (result u32)))
          (core func $plain-core (canon lower (func $plain)))
          (core func $task-return (canon task.return (result u32)))
          (core module $m
            (import "host" "plain" (func $plain (result i32)))
            (import "host" "task-return" (func $ret (param i32)))
            (func (export "run") (result i32) (call $ret (i32.add (call $plain) (i32.const 1))) (i32.const 0))
            (func (export "cb") (param i32 i32 i32) (result i32) unreachable))
          (core instance $i (instantiate $m (with "host" (instance
            (export "plain" (func $plain-core))
            (export "task-return" (func $task-return))))))
          (func (export "run") async (result u32) (canon lift (core func $i "run") async (callback (core func $i "cb")))))
        WAT;

    public function test_an_async_export_with_a_plain_import_returns_its_result(): void
    {
        $exports = (new Instance(new Component(self::ASYNC_EXPORT), ['plain' => fn (): int => 41]))->exports;

        self::assertSame(42, $exports->run());
        self::assertSame(42, $exports->run());
    }

    public function test_a_plain_import_of_an_async_component_may_not_switch_fibers(): void
    {
        $exports = (new Instance(new Component(self::ASYNC_EXPORT), ['plain' => fn (): int => \Fiber::suspend()]))->exports;

        $this->expectException(\FiberError::class);
        (new \Fiber(fn (): int => $exports->run()))->start();
    }

    public function test_the_signature_of_an_async_export_says_async(): void
    {
        $component = new Component(self::ASYNC_EXPORT);

        self::assertSame('async func() -> u32', $component->exports()[0]['type'] ?? null);
    }
}
