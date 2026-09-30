<?php

/**
 * Exports are resolved at runtime, so IDEs cannot see them. A small typed
 * wrapper gives autocompletion and static analysis a contract to check.
 */
final class Counter
{
    private readonly Wasm\Exports $exports;

    public function __construct()
    {
        $this->exports = (new Wasm\Instance(new Wasm\Module(<<<'EOWAT'
            (module
              (global $count (mut i32) (i32.const 0))
              (func (export "increment") (result i32)
                (global.set $count (i32.add (global.get $count) (i32.const 1)))
                (global.get $count)))
            EOWAT)))->exports;
    }

    public function increment(): int
    {
        return $this->exports->increment();
    }
}

$counter = new Counter();
$counter->increment();
var_dump($counter->increment()); // int(2)
