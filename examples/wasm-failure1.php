<?php

$instance = new Wasm\WasmInstance(
    <<<'EOWAT'
    (mosqdkfjsqkdfjsqdfjjqsdfjk
    EOWAT
);

var_dump($instance->add_one(42));
