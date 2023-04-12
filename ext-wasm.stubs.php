<?php

// Stubs for ext-wasm

namespace Wasm {
    class WasmInstance {
        public function __construct(string $wat) {}

        public function __call(string $method, array $attributes): int {}

        public function __get(string $accessor): int {}

        public function __set(string $accessor, int $value): mixed {}
    }
}
