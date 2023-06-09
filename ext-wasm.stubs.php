<?php

// Stubs for ext-wasm

namespace Wasm {
    class InstanceBuilder {
        public static function fromWat(string $wat): \Wasm\InstanceBuilder {}

        public function build(): \Wasm\WasmInstance {}
    }

    class WasmInstance {
        public static function fromBuilder(\Wasm\InstanceBuilder $builder): \Wasm\WasmInstance {}

        public function __call(string $method, array $attributes): mixed {}

        public function __get(string $accessor): mixed {}

        public function __set(string $accessor, mixed $value): mixed {}
    }
}
