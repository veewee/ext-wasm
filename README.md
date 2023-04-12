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
      (type $t0 (func (param i32) (result i32)))
      (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
        get_local $p0
        i32.const 1
        i32.add))
    EOWAT
);

var_dump($instance->add_one(42));
```

```bash
php -d extension=./target/debug/libext_wasm.dylib examples/function.php
```

## Stubs

```
cargo php stubs
```


