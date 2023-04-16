<?php

$instance = Wasm\InstanceBuilder::fromWat(
    <<<'EOWAT'
    (mosqdkfjsqkdfjsqdfjjqsdfjk
    EOWAT
)->build();

var_dump($instance->add_one(42));
