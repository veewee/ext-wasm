<?php

$instance = new Wasm\WasmInstance(
    <<<'EOWAT'
    (module
      (global $one (export "one") i32 (i32.const 1))
      (global $some (export "some") (mut i32) (i32.const 0))
      (func (export "get_one") (result i32) (global.get $one))
      (func (export "get_some") (result i32) (global.get $some))
      (func (export "set_some") (param i32) (global.set $some (local.get 0))))
    EOWAT
);

var_dump($instance->some);
$instance->some = 1;
var_dump($instance->some);
var_dump($instance->set_some(21));
var_dump($instance->get_some());

