<?php

// Shows the link preview of a page and its content as Markdown.
// Usage: php examples/link-preview/preview.php https://www.php.net/ [--read]
// The component may only reach the host and port of the URL given.

declare(strict_types=1);

require __DIR__ . '/LinkPreview.php';

$url = $argv[1] ?? 'https://www.php.net/';
$host = parse_url($url, PHP_URL_HOST);
if (!is_string($host) || $host === '') {
    fwrite(STDERR, "usage: php preview.php https://example.com/page [--read]\n");
    exit(2);
}
// With the port, not just the name, so the component cannot reach other ports on that host.
$port = parse_url($url, PHP_URL_PORT) ?? (parse_url($url, PHP_URL_SCHEME) === 'http' ? 80 : 443);
$pages = new LinkPreview(["$host:$port"]);

try {
    if (in_array('--read', $argv, true)) {
        echo $pages->read($url), "\n";
    } else {
        echo json_encode($pages->preview($url), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES), "\n";
    }
} catch (Wasm\Exception\ComponentError $e) {
    fwrite(STDERR, "failed: {$e->payload}\n");
    exit(1);
}
