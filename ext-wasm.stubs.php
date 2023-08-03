<?php

// Stubs for ext-wasm

namespace Wasm {
    class WasmInstance {
        public static function fromBuilder(\Wasm\InstanceBuilder $builder): \Wasm\WasmInstance {}

        public function __call(string $method, array $attributes): mixed {}

        public function __get(string $accessor): mixed {}

        public function __set(string $accessor, mixed $value): void {}
    }

    class InstanceBuilder {
        public static function fromWat(string $wat): \Wasm\InstanceBuilder {}

        public function import(Imports $imports): void {}

        public function build(): \Wasm\WasmInstance {}
    }

    class Imports {
        public static function create(): self {}

        public static function define(string $namespace, string $variable, \Wasm\Type\Global $value): void {}
    }

}

namespace Wasm\Type {
    class Global {
        public static function mutable(mixed $value): self {}

        public static function immutable(mixed $value): self {}
    }
}
