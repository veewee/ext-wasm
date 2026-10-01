<?php

// Sends a file gzip-compressed while compressing it, behind PHP's built-in server:
//   php -S localhost:8000 examples/stream-gzip/server.php
//   curl --compressed localhost:8000/README.md
// Each compressed chunk is flushed as soon as the component wrote it.

declare(strict_types=1);

require __DIR__ . '/Gzip.php';

$file = realpath(__DIR__ . '/' . ltrim(parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH) ?: '', '/'));
if ($file === false || !str_starts_with($file, __DIR__ . '/') || !is_file($file)) {
    http_response_code(404);
    echo "not found\n";
    return;
}

header('Content-Type: text/plain; charset=utf-8');
header('Content-Encoding: gzip');
foreach ((new Gzip())->compress(Gzip::read($file)) as $chunk) {
    echo $chunk;
    flush();
}
