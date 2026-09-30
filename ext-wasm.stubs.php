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
         * @param array $descriptor
         * @param mixed $value
         */
        public function __construct(array $descriptor, mixed $value = null) {}

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
         */
        public function __construct(\Wasm\Module $module, ?array $imports = null) {}
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
         * @param array $descriptor
         */
        public function __construct(array $descriptor) {}

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
         * @param \Wasm\Module $module
         * @param string $name
         * @return array
         */
        public static function customSections(\Wasm\Module $module, string $name): array {}

        /**
         * @return list<array{name: string, kind: string}>
         *
         * @param \Wasm\Module $module
         * @return array
         */
        public static function exports(\Wasm\Module $module): array {}

        /**
         * @return list<array{module: string, name: string, kind: string}>
         *
         * @param \Wasm\Module $module
         * @return array
         */
        public static function imports(\Wasm\Module $module): array {}
    }

    /**
     * A table of references, like JS `WebAssembly.Table`.
     */
    class Table {
        /**
         * @param array{element: 'anyfunc'|'externref', initial: int, maximum?: int} $descriptor
         *
         * @param array $descriptor
         * @param mixed $value
         */
        public function __construct(array $descriptor, mixed $value = null) {}

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
         * @param array $descriptor
         */
        public function __construct(array $descriptor) {}
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
     * @return mixed
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
