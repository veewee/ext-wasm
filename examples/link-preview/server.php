<?php

// A link preview service behind PHP's built-in server:
//   LINK_PREVIEW_HOSTS='www.php.net,*.wikipedia.org' php -S localhost:8000 examples/link-preview/server.php
//   curl 'localhost:8000/preview?url=https://www.php.net/'
//   curl 'localhost:8000/read?url=https://en.wikipedia.org/wiki/WebAssembly'
// PHP stays in charge of the request, so routing, authentication and caching
// could happen here before the component answers.

declare(strict_types=1);

require __DIR__ . '/LinkPreview.php';

use Wasm\Component\Http\Request;

$hosts = array_filter(explode(',', getenv('LINK_PREVIEW_HOSTS') ?: 'www.php.net,*.wikipedia.org'));
$scheme = (($_SERVER['HTTPS'] ?? '') === 'on') ? 'https' : 'http';
$headers = function_exists('getallheaders') ? getallheaders() : [];

$response = (new LinkPreview($hosts))->handle(new Request(
    $_SERVER['REQUEST_METHOD'],
    "$scheme://{$_SERVER['HTTP_HOST']}{$_SERVER['REQUEST_URI']}",
    $headers,
    file_get_contents('php://input'),
));

http_response_code($response->status);
foreach ($response->headers as $name => $values) {
    foreach ($values as $value) {
        header("$name: $value", false);
    }
}
echo $response->body;
