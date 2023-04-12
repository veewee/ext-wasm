<?php

$instance = new Wasm\AddOneInstance(
    <<<'EOWAT'
    (mosqdkfjsqkdfjsqdfjjqsdfjk
    EOWAT
);

var_dump($instance->addOne(42));
