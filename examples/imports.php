<?php

// Imports use the same shape as the JS import object: module => name => value.
$memory = new Wasm\Memory(['initial' => 1]);
$counter = new Wasm\GlobalVar(['value' => 'i32', 'mutable' => true], 0);

$instance = new Wasm\Instance(new Wasm\Module(<<<'EOWAT'
    (module
      (import "env" "memory" (memory 1))
      (import "env" "counter" (global $counter (mut i32)))
      (import "env" "log" (func $log (param i32 i32)))
      (data (i32.const 0) "Hello from wasm")
      (func (export "run")
        (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
        (call $log (i32.const 0) (i32.const 15))))
    EOWAT), [
    'env' => [
        'memory' => $memory,
        'counter' => $counter,
        // Any PHP callable can be a host function; wasm's arguments arrive as PHP values.
        'log' => function (int $ptr, int $len) use ($memory): void {
            echo $memory->read($ptr, $len), "\n";
        },
    ],
]);

$instance->exports->run(); // Hello from wasm
$instance->exports->run(); // Hello from wasm
var_dump($counter->value); // int(2)
