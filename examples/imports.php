<?php

$instance = Wasm\InstanceBuilder::fromWat(
    <<<'EOWAT'
    (module
      (import "env" "global" (global $global (mut i32)))
      (func (export "read_g") (result i32)
        global.get $global)
      (func (export "write_g") (param i32)
        local.get 0
        global.set $global))
    EOWAT
)->withImports([
  'env' => [
    'global' => 33
  ]
])->build();

var_dump($instance->read_g());
