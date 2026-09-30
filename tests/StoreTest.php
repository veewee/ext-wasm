<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use Wasm\GlobalVar;
use Wasm\Instance;
use Wasm\Memory;
use Wasm\Module;
use Wasm\Store;
use Wasm\Table;
use Wasm\Tag;

final class StoreTest extends TestCase
{
    use RunsPhpInSubprocess;

    private const MATH = '(module (func (export "double") (param i32) (result i32) (i32.mul (local.get 0) (i32.const 2))))';

    private const APPLY = <<<'EOWAT'
        (module
          (import "env" "table" (table 1 funcref))
          (type $unary (func (param i32) (result i32)))
          (func (export "apply") (param i32) (result i32)
            (call_indirect (type $unary) (local.get 0) (i32.const 0))))
        EOWAT;

    public function test_objects_in_one_explicit_store_combine(): void
    {
        $store = new Store();
        $math = new Instance(new Module(self::MATH), store: $store);
        $table = new Table(['element' => 'anyfunc', 'initial' => 1], store: $store);
        $table->set(0, $math->exports->double);
        $apply = new Instance(new Module(self::APPLY), ['env' => ['table' => $table]], $store);

        self::assertSame(14, $apply->exports->apply(7));
    }

    public function test_every_constructor_takes_a_store(): void
    {
        $store = new Store();

        self::assertInstanceOf(Memory::class, new Memory(['initial' => 1], store: $store));
        self::assertInstanceOf(GlobalVar::class, new GlobalVar(['value' => 'i32'], 1, store: $store));
        self::assertInstanceOf(Tag::class, new Tag(['parameters' => []], store: $store));
    }

    public function test_an_instance_joins_the_store_of_its_imports(): void
    {
        $store = new Store();
        $table = new Table(['element' => 'anyfunc', 'initial' => 1], store: $store);
        $math = new Instance(new Module(self::MATH), store: $store);
        $apply = new Instance(new Module(self::APPLY), ['env' => ['table' => $table]]);

        // A LinkError here or in the call would mean $apply did not join $store.
        $table->set(0, $math->exports->double);
        self::assertSame(6, $apply->exports->apply(3));
    }

    /** @return iterable<string, array{string, 1?: string}> */
    public static function mismatches(): iterable
    {
        // Parenthesised, because `new X()->y` needs PHP 8.4.
        $math = '(new Wasm\Instance(new Wasm\Module(\'' . self::MATH . '\')))';

        yield 'import from another store' => [<<<PHP
            \$memory = new Wasm\\Memory(['initial' => 1], store: new Wasm\\Store());
            new Wasm\\Instance(new Wasm\\Module('(module (import "env" "m" (memory 1)))'), ['env' => ['m' => \$memory]], new Wasm\\Store());
            PHP];
        yield 'imports from two stores' => [<<<PHP
            \$memory = new Wasm\\Memory(['initial' => 1], store: new Wasm\\Store());
            \$global = new Wasm\\GlobalVar(['value' => 'i32'], 1);
            new Wasm\\Instance(new Wasm\\Module('(module (import "env" "m" (memory 1)) (import "env" "g" (global i32)))'), ['env' => ['m' => \$memory, 'g' => \$global]]);
            PHP];
        yield 'table set' => [<<<PHP
            \$table = new Wasm\\Table(['element' => 'anyfunc', 'initial' => 1], store: new Wasm\\Store());
            \$table->set(0, {$math}->exports->double);
            PHP];
        yield 'table grow' => [<<<PHP
            \$table = new Wasm\\Table(['element' => 'anyfunc', 'initial' => 1], store: new Wasm\\Store());
            \$table->grow(1, {$math}->exports->double);
            PHP];
        yield 'table initial value' => [<<<PHP
            new Wasm\\Table(['element' => 'anyfunc', 'initial' => 1], {$math}->exports->double, new Wasm\\Store());
            PHP];
        yield 'global value' => [<<<PHP
            \$global = new Wasm\\GlobalVar(['value' => 'anyfunc', 'mutable' => true], null, store: new Wasm\\Store());
            \$global->value = {$math}->exports->double;
            PHP];
        yield 'function argument' => [<<<PHP
            \$id = new Wasm\\Instance(new Wasm\\Module('(module (func (export "id") (param funcref) (result funcref) local.get 0))'), store: new Wasm\\Store());
            \$id->exports->id({$math}->exports->double);
            PHP];
        // A foreign funcref returned by a callback is found inside wasm, where it can only trap.
        yield 'callback result' => [<<<PHP
            \$math = {$math};
            \$get = new Wasm\\Instance(new Wasm\\Module('(module (import "env" "f" (func \$f (result funcref))) (func (export "run") (result funcref) call \$f))'), ['env' => ['f' => fn () => \$math->exports->double]], new Wasm\\Store());
            \$get->exports->run();
            PHP, 'RuntimeError'];
    }

    #[DataProvider('mismatches')]
    public function test_combining_two_stores_is_a_link_error(string $code, string $error = 'LinkError'): void
    {
        $output = $this->runPhp("<?php\ntry {\n{$code}\n    echo 'no error';\n} catch (Throwable \$e) {\n    echo get_class(\$e), ': ', \$e->getMessage();\n}\n");

        self::assertMatchesRegularExpression("/^Wasm\\\\Exception\\\\{$error}: \\w+ belongs to a different store/", $output);
    }

    public function test_a_wasm_throw_with_a_tag_of_another_store_passes_through(): void
    {
        $tag = new Tag(['parameters' => []], store: new Store());
        $run = new Instance(new Module('(module (import "env" "f" (func $f)) (func (export "run") call $f))'), [
            'env' => ['f' => function () use ($tag): void {
                throw new \Wasm\Exception\WasmThrow($tag);
            }],
        ]);

        try {
            $run->exports->run();
            self::fail('Expected the WasmThrow to come back out');
        } catch (\Wasm\Exception\WasmThrow $thrown) {
            self::assertSame($tag, $thrown->tag);
        }
    }

    public function test_a_table_joins_the_store_of_its_initial_value(): void
    {
        $math = new Instance(new Module(self::MATH), store: new Store());
        $table = new Table(['element' => 'anyfunc', 'initial' => 1], $math->exports->double);
        $apply = new Instance(new Module(self::APPLY), ['env' => ['table' => $table]]);

        self::assertSame(14, $apply->exports->apply(7));
    }

    public function test_a_global_joins_the_store_of_its_initial_value(): void
    {
        $math = new Instance(new Module(self::MATH), store: new Store());
        $global = new GlobalVar(['value' => 'anyfunc', 'mutable' => true], $math->exports->double);
        $global->value = $math->exports->double;

        self::assertSame(8, ($global->value)(4));
    }

    public function test_an_externref_initial_value_does_not_pick_the_store(): void
    {
        $memory = new Memory(['initial' => 1], store: new Store());
        $table = new Table(['element' => 'externref', 'initial' => 1], $memory, new Store());
        $global = new GlobalVar(['value' => 'externref'], $memory, store: new Store());

        self::assertSame($memory, $table->get(0));
        self::assertSame($memory, $global->value);
    }

    public function test_standalone_objects_can_be_imported_together(): void
    {
        $memory = new Memory(['initial' => 1]);
        $counter = new GlobalVar(['value' => 'i32', 'mutable' => true], 0);
        $instance = new Instance(new Module(<<<'EOWAT'
            (module
              (import "env" "memory" (memory 1))
              (import "env" "counter" (global $counter (mut i32)))
              (func (export "run")
                (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
                (i32.store8 (i32.const 0) (global.get $counter))))
            EOWAT), ['env' => ['memory' => $memory, 'counter' => $counter]]);

        $instance->exports->run();

        self::assertSame(1, $counter->value);
        self::assertSame("\1", $memory->read(0, 1));
    }

    public function test_memory_stays_flat_with_a_standalone_memory_per_job(): void
    {
        $output = $this->runPhp(<<<'PHP'
            <?php
            $unit = PHP_OS_FAMILY === 'Darwin' ? 1048576 : 1024;
            $module = new Wasm\Module('(module (import "env" "memory" (memory 16)))');
            $kept = new Wasm\Instance(new Wasm\Module('(module (memory 1))'));
            $before = getrusage()['ru_maxrss'] / $unit;
            for ($i = 0; $i < 300; $i++) {
                $memory = new Wasm\Memory(['initial' => 16]);
                $memory->write(0, str_repeat("\1", 1048576));
                $instance = new Wasm\Instance($module, ['env' => ['memory' => $memory]]);
                unset($instance, $memory);
            }
            echo (int) (getrusage()['ru_maxrss'] / $unit - $before);
            PHP);

        self::assertLessThan(100, (int) $output, "peak RSS grew by {$output} MiB");
    }

    public function test_a_callback_cycle_does_not_pin_later_standalone_objects(): void
    {
        $output = $this->runPhp(<<<'PHP'
            <?php
            $unit = PHP_OS_FAMILY === 'Darwin' ? 1048576 : 1024;
            function job(Wasm\Module $module): void {
                $memory = new Wasm\Memory(['initial' => 1]);
                // The callback holds $memory, which holds the store the callback lives in.
                new Wasm\Instance($module, ['env' => ['memory' => $memory, 'f' => function () use ($memory): void {}]]);
            }
            job(new Wasm\Module('(module (import "env" "memory" (memory 1)) (import "env" "f" (func)))'));
            gc_collect_cycles();
            $before = getrusage()['ru_maxrss'] / $unit;
            for ($i = 0; $i < 20; $i++) {
                $memory = new Wasm\Memory(['initial' => 160]);
                $memory->write(0, str_repeat("\1", 160 * 65536));
                unset($memory);
            }
            echo (int) (getrusage()['ru_maxrss'] / $unit - $before);
            PHP);

        // 20 pinned memories of 10 MiB each would grow by about 200 MiB.
        self::assertLessThan(100, (int) $output, "peak RSS grew by {$output} MiB");
    }

    public function test_instances_without_shared_imports_do_not_share_a_store(): void
    {
        $math = new Instance(new Module(self::MATH));
        $id = new Instance(new Module('(module (func (export "id") (param funcref) (result funcref) local.get 0))'));

        $this->expectException(\Wasm\Exception\LinkError::class);
        $this->expectExceptionMessageMatches('/different store/');
        $id->exports->id($math->exports->double);
    }

    public function test_memory_stays_flat_when_one_instance_is_kept_alive(): void
    {
        $output = $this->runPhp(<<<'PHP'
            <?php
            $usage = getrusage();
            if (!isset($usage['ru_maxrss'])) { echo 'skip'; exit; }
            $unit = PHP_OS_FAMILY === 'Darwin' ? 1048576 : 1024;
            $module = new Wasm\Module('(module (memory (export "memory") 16))');
            $kept = new Wasm\Instance($module);
            $before = getrusage()['ru_maxrss'] / $unit;
            for ($i = 0; $i < 300; $i++) {
                $instance = new Wasm\Instance($module);
                $instance->exports->memory->write(0, str_repeat("\1", 1048576));
                unset($instance);
            }
            echo (int) (getrusage()['ru_maxrss'] / $unit - $before);
            PHP);

        if ($output === 'skip') {
            self::markTestSkipped('peak RSS is not available on this platform');
        }
        // 300 kept stores of 1 MiB each would grow by about 300 MiB.
        self::assertLessThan(100, (int) $output, "peak RSS grew by {$output} MiB");
    }

    public function test_an_explicit_store_is_freed_with_its_objects(): void
    {
        $output = $this->runPhp(<<<'PHP'
            <?php
            $unit = PHP_OS_FAMILY === 'Darwin' ? 1048576 : 1024;
            $module = new Wasm\Module('(module (memory (export "memory") 16))');
            $kept = new Wasm\Instance($module);
            $before = getrusage()['ru_maxrss'] / $unit;
            for ($i = 0; $i < 300; $i++) {
                $store = new Wasm\Store();
                $instance = new Wasm\Instance($module, store: $store);
                $instance->exports->memory->write(0, str_repeat("\1", 1048576));
                unset($instance, $store);
            }
            echo (int) (getrusage()['ru_maxrss'] / $unit - $before);
            PHP);

        self::assertLessThan(100, (int) $output, "peak RSS grew by {$output} MiB");
    }
}
