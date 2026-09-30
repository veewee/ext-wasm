<?php

// Renders an invoice to PDF with Typst, from a template and PHP data.
// Usage: php examples/typst/invoice.php [out.pdf]

declare(strict_types=1);

require __DIR__ . '/Typst.php';

$invoice = [
    'number' => '2026-0042',
    'date' => '30 September 2026',
    'seller' => ['name' => 'Acme Web Studio', 'address' => 'Main Street 1, 2000 Antwerp', 'vat' => 'BE0123.456.789', 'iban' => 'BE71 0961 2345 6769'],
    'customer' => ['name' => 'Example Corp', 'address' => 'Market Square 5, 9000 Ghent'],
    'vat_rate' => 0.21,
    'lines' => [
        ['description' => 'Website redesign', 'quantity' => 1, 'price' => 4800],
        ['description' => 'Hosting, per month', 'quantity' => 12, 'price' => 25.5],
        ['description' => 'Support hours', 'quantity' => 6, 'price' => 95],
    ],
];

$typst = new Typst();
$start = hrtime(true);
$pdf = $typst->pdf(file_get_contents(__DIR__ . '/invoice.typ'), $invoice);
$milliseconds = (hrtime(true) - $start) / 1e6;

$file = $argv[1] ?? 'invoice.pdf';
file_put_contents($file, $pdf);
printf("%s: %d bytes in %.0f ms\n", $file, strlen($pdf), $milliseconds);
