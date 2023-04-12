<?php

/**
 * @property int $one
 * @property int $some
 * @method int get_one()
 * @method int get_some()
 * @method int set_some(int $param)
 */
final class MyInstanceDecorator extends Wasm\WasmInstance {
  public function __construct() {
    parent::__construct(
      <<<'EOWAT'
      (module
        (global $one (export "one") i32 (i32.const 1))
        (global $some (export "some") (mut i32) (i32.const 0))
        (func (export "get_one") (result i32) (global.get $one))
        (func (export "get_some") (result i32) (global.get $some))
        (func (export "set_some") (param i32) (global.set $some (local.get 0))))
      EOWAT
    );
  }
}

$instance = new \MyInstanceDecorator();
var_dump($instance->some);
