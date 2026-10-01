<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Exports;
use Wasm\Func;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Store;
use Wasm\Suspending;
use Wasm\Table;
use Wasm\Tag;
use Wasm\Exception\WasmThrow;

/** A Wasm\Func made from a function type and a PHP callable, like JS `WebAssembly.Function`. */
final class HostFuncTest extends TestCase
{
    private const ADD = ['parameters' => ['i32', 'i32'], 'results' => ['i32']];

    private static function add(): Func
    {
        return new Func(self::ADD, fn (int $a, int $b): int => $a + $b);
    }

    private static function exports(string $body, array $imports = []): Exports
    {
        return (new Instance(new Module("(module $body)"), $imports))->exports;
    }

    public function test_it_is_a_func_of_the_given_type(): void
    {
        $add = self::add();

        self::assertSame(self::ADD, $add->type());
        self::assertSame(2, $add->length());
        self::assertSame(5, $add(2, 3));
    }

    public function test_calling_it_from_php_converts_like_a_wasm_function(): void
    {
        $add = self::add();

        self::assertSame(-2, $add(0x7fffffff, 0x7fffffff), 'the result wraps as an i32');
        $this->expectException(\ArgumentCountError::class);
        $add(1);
    }

    public function test_wasm_calls_it_through_a_table(): void
    {
        $exports = self::exports('
            (type $add (func (param i32 i32) (result i32)))
            (table (export "table") 1 funcref)
            (func (export "run") (param i32 i32) (result i32)
              (call_indirect (type $add) (local.get 0) (local.get 1) (i32.const 0)))');

        $exports->table->set(0, self::add());

        self::assertSame(7, $exports->run(3, 4));
    }

    public function test_a_table_created_from_php_holds_it(): void
    {
        $table = new Table(['element' => 'anyfunc', 'initial' => 1], self::add());
        $exports = self::exports('
            (type $add (func (param i32 i32) (result i32)))
            (import "env" "table" (table 1 funcref))
            (func (export "run") (result i32)
              (call_indirect (type $add) (i32.const 20) (i32.const 22) (i32.const 0)))', ['env' => ['table' => $table]]);

        self::assertSame(42, $exports->run());
        self::assertInstanceOf(Func::class, $table->get(0));
        self::assertSame(9, $table->get(0)(4, 5));
    }

    public function test_wasm_calls_it_as_a_funcref_argument(): void
    {
        $exports = self::exports('
            (type $add (func (param i32 i32) (result i32)))
            (func (export "apply") (param funcref) (result i32)
              (call_ref $add (i32.const 1) (i32.const 2) (ref.cast (ref $add) (local.get 0))))');

        self::assertSame(3, $exports->apply(self::add()));
    }

    public function test_it_matches_a_final_function_type_of_the_module(): void
    {
        $exports = self::exports('
            (type $add (func (param i32 i32) (result i32)))
            (func (export "apply") (param (ref $add)) (result i32)
              (call_ref $add (i32.const 5) (i32.const 6) (local.get 0)))');

        self::assertSame(11, $exports->apply(self::add()));
    }

    public function test_it_does_not_match_an_open_subtype_of_the_module(): void
    {
        $exports = self::exports('
            (type $add (sub (func (param i32 i32) (result i32))))
            (func (export "apply") (param (ref $add)) (result i32)
              (call_ref $add (i32.const 5) (i32.const 6) (local.get 0)))');

        $this->expectException(\TypeError::class);
        $exports->apply(self::add());
    }

    public function test_one_func_works_in_several_instances(): void
    {
        $add = self::add();
        $body = '(import "env" "add" (func $add (param i32 i32) (result i32)))
            (func (export "run") (result i32) (call $add (i32.const 1) (i32.const 1)))';

        self::assertSame(2, self::exports($body, ['env' => ['add' => $add]])->run());
        self::assertSame(2, self::exports($body, ['env' => ['add' => $add]])->run());
    }

    public function test_it_calls_itself_from_its_callback(): void
    {
        $countdown = null;
        $countdown = new Func(['parameters' => ['i32'], 'results' => ['i32']], function (int $n) use (&$countdown): int {
            return $n === 0 ? 0 : 1 + $countdown($n - 1);
        });
        $exports = self::exports('
            (import "env" "f" (func $f (param i32) (result i32)))
            (func (export "run") (result i32) (call $f (i32.const 3)))', ['env' => ['f' => $countdown]]);

        self::assertSame(3, $exports->run());
        $countdown = null;
    }

    public function test_a_wasm_exception_from_the_callback_is_caught_by_wasm(): void
    {
        $tag = new Tag(['parameters' => ['i32']]);
        $thrower = new Func(['parameters' => [], 'results' => []], fn () => throw new WasmThrow($tag, [42]));
        $exports = self::exports('
            (import "env" "tag" (tag $t (param i32)))
            (import "env" "throw" (func $throw))
            (func (export "run") (result i32)
              (block $caught (result i32)
                (try_table (catch $t $caught) (call $throw))
                (i32.const -1)))', ['env' => ['tag' => $tag, 'throw' => $thrower]]);

        self::assertSame(42, $exports->run());
    }

    public function test_an_exception_from_the_callback_reaches_the_php_caller(): void
    {
        $thrown = new \DomainException('from php');
        $f = new Func(['parameters' => [], 'results' => []], fn () => throw $thrown);

        try {
            $f();
            self::fail('expected the exception');
        } catch (\DomainException $caught) {
            self::assertSame($thrown, $caught);
        }

        $exports = self::exports('(import "env" "f" (func $f)) (func (export "run") (call $f))', ['env' => ['f' => $f]]);
        $this->expectExceptionObject($thrown);
        $exports->run();
    }

    public function test_a_wasm_exception_thrown_from_a_direct_call_reaches_php(): void
    {
        $tag = new Tag(['parameters' => ['i32']]);
        $f = new Func(['parameters' => [], 'results' => []], fn () => throw new WasmThrow($tag, [7]));

        try {
            $f();
            self::fail('expected a WasmThrow');
        } catch (WasmThrow $thrown) {
            self::assertSame([7], $thrown->payload);
        }
    }

    public function test_a_wrong_return_type_fails_as_for_an_import(): void
    {
        $f = new Func(['parameters' => [], 'results' => ['i32']], fn () => 'not an int');

        $this->expectException(\Wasm\Exception\RuntimeError::class);
        $this->expectExceptionMessage('expected int for i32, got string');
        $f();
    }

    public function test_a_func_may_wrap_another_func(): void
    {
        $wrapped = new Func(self::ADD, self::add());

        self::assertSame(5, $wrapped(2, 3));
    }

    public function test_it_runs_in_a_store_with_suspending_imports(): void
    {
        $store = new Store();
        $exports = (new Instance(new Module('(module
            (import "env" "wait" (func $wait (result i32)))
            (import "env" "add" (func $add (param i32 i32) (result i32)))
            (func (export "run") (result i32) (call $add (call $wait) (i32.const 1))))'), ['env' => [
            'wait' => new Suspending(fn (): int => \Fiber::suspend()),
            'add' => self::add(),
        ]], $store))->exports;

        $fiber = new \Fiber(fn (): int => $exports->run());
        $fiber->start();
        $fiber->resume(41);

        self::assertSame(42, $fiber->getReturn());
    }

    /** @return iterable<string, array{mixed, string}> */
    public static function badTypes(): iterable
    {
        yield 'no parameters' => [['results' => []], 'parameters'];
        yield 'no results' => [['parameters' => []], 'results'];
        yield 'parameters not a list' => [['parameters' => 'i32', 'results' => []], 'parameters'];
        yield 'an unknown value type' => [['parameters' => ['i33'], 'results' => []], 'i33'];
        yield 'a type PHP cannot hold' => [['parameters' => ['anyref'], 'results' => []], 'anyref'];
    }

    #[\PHPUnit\Framework\Attributes\DataProvider('badTypes')]
    public function test_a_bad_type_is_a_type_error(array $type, string $message): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage($message);

        new Func($type, fn () => null);
    }

    public function test_a_suspending_callback_is_a_type_error(): void
    {
        $this->expectException(\TypeError::class);

        new Func(['parameters' => [], 'results' => []], new Suspending(fn () => null));
    }

    public function test_its_type_goes_back_in(): void
    {
        $add = self::add();

        self::assertSame(self::ADD, (new Func($add->type(), fn (int $a, int $b): int => 0))->type());
        $exported = self::exports('(func (export "f") (param f64 funcref) (result i64 externref) unreachable)')->f;
        self::assertSame($exported->type(), (new Func($exported->type(), fn () => null))->type());
    }
}
