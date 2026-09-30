<?php

// Checks orders with the same JavaScript rules the browser uses (js/rules.js),
// run in QuickJS through WASI. Usage: php examples/quickjs/checkout.php

declare(strict_types=1);

/** @return array{errors: list<string>}|array{totals: array{net: int, vat: int, gross: int}} */
function checkout(Wasm\Module $quickjs, array $order): array
{
    // The script sees only the js directory, read-only, and the order on stdin.
    $wasi = new Wasm\Wasi(
        args: ['qjs', '/app/main.js'],
        stdin: json_encode($order, JSON_THROW_ON_ERROR),
        preopens: ['/app' => __DIR__ . '/js'],
    );
    $exitCode = $wasi->start(new Wasm\Instance($quickjs, $wasi->getImportObject()));
    if ($exitCode !== 0) {
        throw new RuntimeException("The rules failed with exit code $exitCode: " . $wasi->stderr());
    }

    return json_decode($wasi->stdout(), true, flags: JSON_THROW_ON_ERROR);
}

$quickjs = new Wasm\Module(file_get_contents(__DIR__ . '/dist/qjs.wasm'));

$orders = [
    'valid order' => [
        'country' => 'BE',
        'email' => 'jane@example.com',
        'lines' => [['sku' => 'mug', 'price' => 1299, 'quantity' => 2], ['sku' => 'tea', 'price' => 450, 'quantity' => 1]],
    ],
    'invalid order' => [
        'country' => 'US',
        'email' => 'not an email',
        'lines' => [['sku' => 'mug', 'price' => 1299, 'quantity' => 0]],
    ],
];

foreach ($orders as $name => $order) {
    echo $name, ': ', json_encode(checkout($quickjs, $order), JSON_PRETTY_PRINT), "\n";
}
