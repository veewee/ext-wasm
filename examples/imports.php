<?php

$instanceBuilder = Wasm\InstanceBuilder::fromWat(
  <<<'EOWAT'
  (module
    (import "env" "global" (global $global (mut i32)))
    (func (export "read_g") (result i32)
      global.get $global)
    (func (export "write_g") (param i32)
      local.get 0
      global.set $global))
  EOWAT
);

$imports = Wasm\Imports::create();
$imports->define('env', 'global', \Wasm\Type\Global::mutable(32));
$instanceBuilder->import($imports);

$instance = $instanceBuilder->build();

var_dump($instance->read_g());
