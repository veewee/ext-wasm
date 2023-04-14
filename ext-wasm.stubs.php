<?php

// Stubs for ext-wasm

namespace Wasm {
    class WasmInstance {
        public function __construct(string $wat) {}

        public function __call(string $method, array $attributes): mixed {}

        public function __get(string $accessor): mixed {}

        public function __set(string $accessor, mixed $value): mixed {}
    }
}
