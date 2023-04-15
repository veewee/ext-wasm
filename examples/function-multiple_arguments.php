<?php

$instance = new Wasm\WasmInstance(
  <<<'EOWAT'
    (module
      (func $swap (export "swap") (param i32 i32) (result i32 i32)
        (local.get 1) (local.get 0)
      )
    )
    EOWAT
);

var_dump($instance->swap(1, 2));