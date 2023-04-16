<?php

/**
 * @property int $one
 * @property int $some
 * @method int get_one()
 * @method int get_some()
 * @method int set_some(int $param)
 */
final class MyInstanceProxy {
  private readonly Wasm\WasmInstance $proxy;

  public function __construct() {

    $this->proxy = Wasm\InstanceBuilder::fromWat(
      <<<'EOWAT'
      (module
        (global $one (export "one") i32 (i32.const 1))
        (global $some (export "some") (mut i32) (i32.const 0))
        (func (export "get_one") (result i32) (global.get $one))
        (func (export "get_some") (result i32) (global.get $some))
        (func (export "set_some") (param i32) (global.set $some (local.get 0))))
      EOWAT
    )->build();
  }

  public function __call(string $name, array $args): mixed
  {
    return $this->proxy->__call($name, $args);
  }

  public function __get(string $name): mixed
  {
    return $this->proxy->__get($name);
  }

  public function __set(string $name, mixed $value): void
  {
    $this->proxy->__set($name, $$value);
  }
}

$instance = new \MyInstanceProxy();
var_dump($instance->some);
