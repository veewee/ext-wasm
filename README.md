# WASM Extension for PHP

:exclamation::exclamation::exclamation: EXPERIMENTAL  :exclamation::exclamation::exclamation:


## Installation

**Prerequisites:**

* rust toolchain
* php >= 8.0 (phpdev, php-cli, php-config)
* clang


```sh
make install
```

## Usage

```php
$instance = Wasm\InstanceBuilder::fromWat(
    <<<'EOWAT'
    (module
      (global $some (export "some") (mut i32) (i32.const 0))
      (func (export "get_some") (result i32) (global.get $some))
      (func (export "set_some") (param i32) (global.set $some (local.get 0))))
    EOWAT
)->build();

var_dump($instance->some);
$instance->some = 1;
var_dump($instance->some);
var_dump($instance->set_some(21));
var_dump($instance->get_some());
```

```bash
php examples/global.php
```

Currently, this package supports `global` access and function `exports`.

## Stubs

```
cargo php stubs
```

## Roadmap

[You can find a more detailed roadmap here.](https://github.com/users/veewee/projects/1)
Feel free to give me some additional keyboards! :)

