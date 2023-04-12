<?php

$instance = new Wasm\AddOneInstance(
    <<<'EOWAT'
    (module
      (type $t0 (func (param i32) (result i32)))
      (func $add_two (export "add_two") (type $t0) (param $p0 i32) (result i32)
        get_local $p0
        i32.const 1
        i32.add))
    EOWAT
);

var_dump($instance->addOne(42));
