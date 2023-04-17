<?php

$instance = Wasm\InstanceBuilder::fromWat(
  <<<'EOWAT'
    (module
      (func $swap (export "swap") (param i32 i32) (result i32 i32)
        (local.get 1) (local.get 0)
      )
    )
    EOWAT
)->build();

var_dump($instance->swap(1, 2));
