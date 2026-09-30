<?php

$exports = (new Wasm\Instance(new Wasm\Module(<<<'EOWAT'
    (module
      (global $one (export "one") i32 (i32.const 1))
      (global $some (export "some") (mut i32) (i32.const 0))
      (func (export "get_some") (result i32) (global.get $some))
      (func (export "set_some") (param i32) (global.set $some (local.get 0))))
    EOWAT)))->exports;

var_dump($exports->some->value); // int(0)
$exports->some->value = 1;
var_dump($exports->get_some()); // int(1)
$exports->set_some(21);
var_dump($exports->some->value); // int(21)

try {
    $exports->one->value = 2;
} catch (TypeError $error) {
    echo $error->getMessage(), "\n"; // cannot set the value of an immutable global
}
