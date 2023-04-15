# 0.1 functions and globals

- [ ] support all scalar types for functions and vars
- [ ] provide custom exception class(es) - might require https://github.com/davidcole1340/ext-php-rs/pull/214
- [ ] allow multiple return types on __call()
- [ ] add extension tests
- [ ] check if `examples/function-multiple_arguments.php` fails because of user error or if it is an issue with `ext-php-rs`
- [x] add github actions workflow
- [ ] see if we can provide type-metadata to build proxies
- [ ] see if we can make decorators work (inheritance crashes the system) - might not work because of https://github.com/davidcole1340/ext-php-rs/issues/203#issuecomment-1328057539
- [ ] make sure stubs are generated correctly 
- [ ] documentation 

# 0.x broaden wasmer features support

- [ ] support multiple encoders (singlepass, cranelift, llvm)
- [ ] provide an `InstanceBuilder` for configuring how an instance is set up
- [ ] support for Webassembly imports
- [ ] support for Webassembly memory
- [ ] support for Webassembly tables
- [ ] support ffor WASI
- [ ] performance: "native" dylib engine (see python)

Example:

```php
$instance = Wasm\InstanceBuilder::fromWat(
    <<<'EOWAT'
    (module
      (global $one (export "one") i32 (i32.const 1))
      (global $some (export "some") (mut i32) (i32.const 0))
      (func (export "get_one") (result i32) (global.get $one))
      (func (export "get_some") (result i32) (global.get $some))
      (func (export "set_some") (param i32) (global.set $some (local.get 0))))
    EOWAT
)
    ->withEngine(Wasm\Engine::UNIVERSAL)
    ->withCompiler(Wasm\Compiler::LLVM)
    ->withImports($imports)
    ->build();

// OR

$instance = Wasm\InstanceBuilder::fromWasi(read_file($binaryFile))->build();

```

# easy of use

- [ ] check if we can provide for bindings for people who don't want to install the extension
- [ ] investigate how to provide pecl/pear
- [ ] provide pre-compiled versions for any OS?
- [ ] makefile for more regular way of installing extension
