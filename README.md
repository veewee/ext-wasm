# WASM Extension for PHP

:exclamation::exclamation::exclamation: EXPERIMENTAL  :exclamation::exclamation::exclamation:


## Installation

```sh
cargo build
cargo php install
```

## Usage

```php
$instance = new Wasm\WasmInstance(
    <<<'EOWAT'
    (module
      (global $some (export "some") (mut i32) (i32.const 0))
      (func (export "get_some") (result i32) (global.get $some))
      (func (export "set_some") (param i32) (global.set $some (local.get 0))))
    EOWAT
);

var_dump($instance->some);
$instance->some = 1;
var_dump($instance->some);
var_dump($instance->set_some(21));
var_dump($instance->get_some());
```

```bash
php -d extension=./target/debug/libext_wasm.dylib examples/global.php
```

Currently, this package supports `global` access and function `exports`.

## Stubs

```
cargo php stubs
```

## Roadmap

[You can find a more detailed roadmap here.](ROADMAP.md)
Feel free to give me some additional keyboards! :)

