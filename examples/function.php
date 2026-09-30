<?php

$instance = new Wasm\Instance(new Wasm\Module(<<<'EOWAT'
    (module
      (func (export "add_one") (param i32) (result i32)
        local.get 0
        i32.const 1
        i32.add))
    EOWAT));

var_dump($instance->exports->add_one(42)); // int(43)

// Exports are Wasm\Func objects, which PHP can call like any callable.
$addOne = $instance->exports->add_one;
var_dump(array_map($addOne, [1, 2, 3])); // [2, 3, 4]
