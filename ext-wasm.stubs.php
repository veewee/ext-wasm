<?php

// Stubs for wasm

namespace Wasm {
    /**
     * The exports of an instance, like JS `instance.exports`.
     *
     * Wrapper objects are created once, so `$exports->f === $exports->f` holds as in JS.
     */
    class Exports implements \Iterator {
        /**
         * @param string $name
         * @param array $arguments
         * @return mixed
         */
        public function __call(string $name, array $arguments): mixed {}

        public function __construct() {}

        /**
         * @param string $name
         * @return mixed
         */
        public function __get(string $name): mixed {}

        /**
         * @param string $name
         * @return bool
         */
        public function __isset(string $name): bool {}

        /**
         * @return mixed
         */
        public function current(): mixed {}

        /**
         * @return string|null
         */
        public function key(): ?string {}

        /**
         * @return void
         */
        public function next(): void {}

        /**
         * @return void
         */
        public function rewind(): void {}

        /**
         * @return bool
         */
        public function valid(): bool {}
    }

    /**
     * An exported wasm function, callable from PHP.
     */
    class Func {
        public function __construct() {}

        /**
         * @param mixed $args
         * @return mixed
         */
        public function __invoke(mixed ...$args): mixed {}

        /**
         * Number of parameters, like JS `Function.prototype.length`.
         *
         * @return int
         */
        public function length(): int {}
    }

    /**
     * A wasm global, like JS `WebAssembly.Global`. Named GlobalVar because
     * `Global` is a reserved word in PHP.
     *
     * @property mixed $value
     */
    class GlobalVar {
        /**
         * @param array{value: string, mutable?: bool} $descriptor
         *
         * @param mixed $value
         * @param \Wasm\Store|null $store
         */
        public function __construct(array $descriptor, mixed $value = null, ?\Wasm\Store $store = null) {}

        /**
         * @param string $name
         * @return mixed
         */
        public function __get(string $name): mixed {}

        /**
         * @param string $name
         * @return bool
         */
        public function __isset(string $name): bool {}

        /**
         * @param string $name
         * @param mixed $value
         * @return void
         */
        public function __set(string $name, mixed $value): void {}

        /**
         * @return mixed
         */
        public function valueOf(): mixed {}
    }

    class Instance {
        public readonly mixed $exports = null;

        /**
         * @param \Wasm\Module $module
         * @param array|null $imports
         * @param \Wasm\Store|null $store
         */
        public function __construct(\Wasm\Module $module, ?array $imports = null, ?\Wasm\Store $store = null) {}
    }

    /**
     * Linear memory, like JS `WebAssembly.Memory`.
     *
     * PHP has no shared ArrayBuffer, so reads and writes copy bytes in and out.
     */
    class Memory {
        /**
         * @param array{initial: int, maximum?: int} $descriptor
         *
         * @param \Wasm\Store|null $store
         */
        public function __construct(array $descriptor, ?\Wasm\Store $store = null) {}

        /**
         * A copy of the whole memory.
         *
         * @return string
         */
        public function buffer(): string {}

        /**
         * @return int
         */
        public function byteLength(): int {}

        /**
         * Grows the memory by `delta` pages and returns the previous size in pages.
         *
         * @param int $delta
         * @return int
         */
        public function grow(int $delta): int {}

        /**
         * @param int $offset
         * @param int $length
         * @return string
         */
        public function read(int $offset, int $length): string {}

        /**
         * @param int $offset
         * @param string $data
         * @return void
         */
        public function write(int $offset, string $data): void {}
    }

    class Module {
        /**
         * Compiles a wasm binary or WAT text.
         *
         * @param string $bytes
         */
        public function __construct(string $bytes) {}

        /**
         * @return list<string>
         *
         * @param string $name
         */
        public function customSections(string $name): array {}

        /**
         * @return list<array{name: string, kind: string}>
         */
        public function exports(): array {}

        /**
         * Compiles a wasm binary or WAT file, like `new Module(file_get_contents($path))`.
         *
         * Reads local files only and honours open_basedir. Use file_get_contents()
         * for stream wrappers such as phar:// or compress.zlib://.
         *
         * @param string $path
         * @return \Wasm\Module
         */
        public static function fromFile(string $path): \Wasm\Module {}

        /**
         * @return list<array{module: string, name: string, kind: string}>
         */
        public function imports(): array {}
    }

    /**
     * Groups wasm objects so they can be combined.
     *
     * An object created without a store joins the store of the wasm objects it
     * is built from. Otherwise an instance gets a store of its own, and a
     * Memory, Table, GlobalVar or Tag joins the store all such standalone
     * objects share. wasmtime frees memory one whole store at a time, when no
     * object in it is left.
     */
    class Store {
        public function __construct() {}
    }

    /**
     * Marks a function import that may suspend the calling Fiber, like JS
     * `WebAssembly.Suspending`.
     *
     * Every other Fiber keeps running while wasm waits for the callback. Calling
     * into the instance's store again before the callback returns throws a
     * RuntimeError, so run one instance per Fiber.
     */
    class Suspending {
        /**
         * @param mixed $callback
         */
        public function __construct(mixed $callback) {}
    }

    /**
     * A table of references, like JS `WebAssembly.Table`.
     */
    class Table {
        /**
         * @param array{element: 'anyfunc'|'externref', initial: int, maximum?: int} $descriptor
         *
         * @param mixed $value
         * @param \Wasm\Store|null $store
         */
        public function __construct(array $descriptor, mixed $value = null, ?\Wasm\Store $store = null) {}

        /**
         * @param int $index
         * @return mixed
         */
        public function get(int $index): mixed {}

        /**
         * Grows the table by `delta` entries and returns the previous length.
         *
         * @param int $delta
         * @param mixed $value
         * @return int
         */
        public function grow(int $delta, mixed $value = null): int {}

        /**
         * @return int
         */
        public function length(): int {}

        /**
         * @param int $index
         * @param mixed $value
         * @return void
         */
        public function set(int $index, mixed $value = null): void {}
    }

    /**
     * An exception tag, like JS `WebAssembly.Tag`.
     */
    class Tag {
        /**
         * @param array{parameters: list<string>} $descriptor
         *
         * @param \Wasm\Store|null $store
         */
        public function __construct(array $descriptor, ?\Wasm\Store $store = null) {}
    }

    /**
     * A WASI preview1 environment for one run of one module, like Node's `WASI`.
     *
     * Nothing of the host is visible to the module except what is passed here:
     * no environment, no stdio and no files outside the preopened directories.
     * stdout and stderr are captured and read after the run.
     */
    class Wasi {
        /**
         * @param list<string>|null $args argv, including the program name
         * @param array<string, string>|null $env
         * @param array<string, string|array{path: string, writable?: bool}>|null $preopens guest path => host path
         * @param int|null $outputLimit bytes kept of stdout and of stderr, 16 MiB by default
         *
         * @param string|null $stdin
         */
        public function __construct(?array $args = null, ?array $env = null, ?array $preopens = null, ?string $stdin = null, ?int $outputLimit = null) {}

        /**
         * @return array{wasi_snapshot_preview1: array<string, \Wasm\Func>}
         */
        public function getImportObject(): array {}

        /**
         * Runs `_initialize` when the module exports it, for modules used as a library.
         *
         * @param \Wasm\Instance $instance
         * @return void
         */
        public function initialize(\Wasm\Instance $instance): void {}

        /**
         * Runs `_start` and returns the exit code.
         *
         * @param \Wasm\Instance $instance
         * @return int
         */
        public function start(\Wasm\Instance $instance): int {}

        /**
         * @return string
         */
        public function stderr(): string {}

        /**
         * @return string
         */
        public function stdout(): string {}
    }

    /**
     * @param string $bytes
     * @return \Wasm\Module
     */
    function compile(string $bytes): \Wasm\Module {}

    /**
     * Like JS `WebAssembly.instantiate()`: bytes give `['module' => Module, 'instance' => Instance]`,
     * a Module gives the Instance.
     *
     * @return Instance|array{module: Module, instance: Instance}
     *
     * @param mixed $source
     * @param array|null $imports
     */
    function instantiate(mixed $source, ?array $imports = null): mixed {}

    /**
     * Whether `bytes` is a valid wasm binary or WAT module.
     *
     * @param string $bytes
     * @return bool
     */
    function validate(string $bytes): bool {}
}

namespace Wasm\Component {
    /**
     * A compiled WebAssembly component.
     *
     * Compile once and instantiate as often as needed, like `Wasm\Module`.
     */
    class Component {
        /**
         * Compiles a component binary or WAT text.
         *
         * @param string $bytes
         */
        public function __construct(string $bytes) {}

        /**
         * @return list<array{name: string, kind: string, type?: string, functions?: list<array{name: string, kind: string, type?: string}>}>
         */
        public function exports(): array {}

        /**
         * Compiles a component file, like `new Component(file_get_contents($path))`.
         *
         * Reads local files only and honours open_basedir.
         *
         * @param string $path
         * @return \Wasm\Component\Component
         */
        public static function fromFile(string $path): \Wasm\Component\Component {}

        /**
         * @return list<array{name: string, kind: string, type?: string, functions?: list<array{name: string, kind: string, type?: string}>}>
         */
        public function imports(): array {}
    }
}

namespace Wasm\Exception {
    class CompileError extends \Wasm\Exception\WasmException {
        /**
         * @param string|null $message
         * @param int|null $code
         * @param mixed $previous
         */
        public function __construct(?string $message = null, ?int $code = null, mixed $previous = null) {}
    }

    class LinkError extends \Wasm\Exception\WasmException {
        /**
         * @param string|null $message
         * @param int|null $code
         * @param mixed $previous
         */
        public function __construct(?string $message = null, ?int $code = null, mixed $previous = null) {}
    }

    class RuntimeError extends \Wasm\Exception\WasmException {
        /**
         * @param string|null $message
         * @param int|null $code
         * @param mixed $previous
         */
        public function __construct(?string $message = null, ?int $code = null, mixed $previous = null) {}
    }

    class WasmException extends \Exception {
        /**
         * @param string|null $message
         * @param int|null $code
         * @param mixed $previous
         */
        public function __construct(?string $message = null, ?int $code = null, mixed $previous = null) {}
    }

    /**
     * A wasm exception, like JS `WebAssembly.Exception`.
     *
     * Thrown in PHP when a wasm exception escapes to PHP, and thrown by a PHP
     * callback to raise an exception that wasm code can catch.
     *
     * @property \Wasm\Tag $tag
     * @property list<mixed> $payload
     */
    class WasmThrow extends \Wasm\Exception\WasmException {
        public $payload = null;

        public $tag = null;

        /**
         * @param \Wasm\Tag $tag
         * @param array|null $payload
         */
        public function __construct(\Wasm\Tag $tag, ?array $payload = null) {}
    }
}
