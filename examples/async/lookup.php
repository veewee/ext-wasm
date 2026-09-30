<?php

declare(strict_types=1);

/*
 * Ten wasm instances each ask PHP for a slow lookup through a Suspending
 * import. Every lookup waits 0.1 s with Amp\delay(), standing in for an HTTP
 * call, and the ten wait together: the run takes about 0.1 s, not 1 s, while
 * a ticker keeps running on the event loop.
 */

require __DIR__ . '/vendor/autoload.php';

use Revolt\EventLoop;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Suspending;

use function Amp\async;
use function Amp\delay;
use function Amp\Future\await;

// greet(id) asks the host to write the name of `id` into memory at 7, right
// after "Hello, ", and returns the length of the greeting it assembles at 0.
$module = new Module(<<<'EOWAT'
    (module
      (import "host" "lookup" (func $lookup (param i32 i32 i32) (result i32)))
      (memory (export "memory") 1)
      (data (i32.const 0) "Hello, ")
      (func (export "greet") (param $id i32) (result i32)
        (local $len i32)
        (local.set $len (call $lookup (local.get $id) (i32.const 7) (i32.const 48)))
        (i32.store8 (i32.add (i32.const 7) (local.get $len)) (i32.const 33))
        (i32.add (local.get $len) (i32.const 8))))
    EOWAT);

$names = ['Ada', 'Grace', 'Linus', 'Barbara', 'Ken', 'Margaret', 'Dennis', 'Frances', 'Guido', 'Rasmus'];

$ticks = 0;
$ticker = EventLoop::repeat(0.01, function () use (&$ticks): void {
    $ticks++;
});

$started = microtime(true);
$greetings = await(array_map(function (int $id) use ($module, $names) {
    return async(function () use ($id, $module, $names): string {
        $instance = null;
        $instance = new Instance($module, ['host' => ['lookup' => new Suspending(
            function (int $id, int $ptr, int $cap) use (&$instance, $names): int {
                delay(0.1);
                $name = substr($names[$id], 0, $cap);
                $instance->exports->memory->write($ptr, $name);

                return strlen($name);
            },
        )]]);
        $length = $instance->exports->greet($id);

        return $instance->exports->memory->read(0, $length);
    });
}, array_keys($names)));
$elapsed = microtime(true) - $started;
EventLoop::cancel($ticker);

echo implode("\n", $greetings), "\n";
printf("%d lookups of 0.1 s took %.2f s, the ticker ran %d times meanwhile\n", count($greetings), $elapsed, $ticks);
echo $elapsed < 0.5 ? "concurrent\n" : "sequential\n";
