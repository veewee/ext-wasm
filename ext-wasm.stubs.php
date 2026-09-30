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

    class Instance {
        public readonly mixed $exports = null;

        /**
         * @param \Wasm\Module $module
         * @param array|null $imports
         */
        public function __construct(\Wasm\Module $module, ?array $imports = null) {}
    }

    class Module {
        /**
         * Compiles a wasm binary or WAT text.
         *
         * @param string $bytes
         */
        public function __construct(string $bytes) {}
    }
}

namespace Wasm\Exception {
    class CompileError extends \Wasm\Exception\WasmException {
        public function __construct() {}
    }

    class LinkError extends \Wasm\Exception\WasmException {
        public function __construct() {}
    }

    class RuntimeError extends \Wasm\Exception\WasmException {
        public function __construct() {}
    }

    class WasmException extends \Exception {
        public function __construct() {}
    }
}
