<?php

$instance = new Wasm\Instance(new Wasm\Module(<<<'EOWAT'
    (module
      (func (export "swap") (param i32 i32) (result i32 i32)
        (local.get 1) (local.get 0)))
    EOWAT));

// Several results come back as a list.
var_dump($instance->exports->swap(1, 2)); // [2, 1]
