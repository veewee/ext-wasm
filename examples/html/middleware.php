<?php

// Cleans a user comment and rewrites the page it lands on, the way a response
// middleware would: lazy images, a CSP nonce on every script, and safe
// external links. Usage: php examples/html/middleware.php [page.html]

declare(strict_types=1);

require __DIR__ . '/Html.php';

$html = new Html();

$comment = '<p onclick="steal()">Nice post! <script>alert(1)</script><a href="javascript:steal()">win</a> <b>really</b></p>';
$page = isset($argv[1])
    ? file_get_contents($argv[1])
    : <<<HTML
        <html>
          <head><script src="/app.js"></script></head>
          <body>
            <img src="/hero.jpg" alt="Hero">
            <div class="ad">Buy now</div>
            <a href="https://example.com">elsewhere</a> <a href="/about">about</a>
            <section class="comments">{$html->sanitize($comment)}</section>
          </body>
        </html>
        HTML;

$nonce = base64_encode(random_bytes(12));

echo $html->rewrite($page, [
    ['selector' => 'img', 'set' => ['loading' => 'lazy', 'decoding' => 'async']],
    // The nonce vouches for every script in the page, so untrusted markup has
    // to go through sanitize() before it reaches this rule.
    ['selector' => 'script', 'set' => ['nonce' => $nonce]],
    ['selector' => 'a[href^="http"]', 'set' => ['rel' => 'noopener', 'target' => '_blank']],
    ['selector' => '.ad', 'remove' => true],
]);
echo "\nContent-Security-Policy: script-src 'nonce-{$nonce}'\n";
