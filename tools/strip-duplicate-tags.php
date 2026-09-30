<?php

// cargo-php appends its own "@param" and "@return" tags to every docblock, also
// when the doc comment already documents a more precise type. The first tag,
// written in the doc comment, is kept.

declare(strict_types=1);

$file = $argv[1];
$stubs = preg_replace_callback('~/\*\*.*?\*/~s', static function (array $match): string {
    $seen = [];
    $lines = [];
    foreach (explode("\n", $match[0]) as $line) {
        // Types never contain "$", so the first "$name" after @param is the parameter.
        if (preg_match('~^\s*\* @(param\s[^$]*(\$\w+)|return\s)~', $line, $tag)) {
            $key = isset($tag[2]) && $tag[2] !== '' ? 'param ' . $tag[2] : 'return';
            if (isset($seen[$key])) {
                continue;
            }
            $seen[$key] = true;
        }
        $lines[] = $line;
    }

    return preg_replace('~\n(\s*\*\n)+(\s*\*/)$~', "\n$2", implode("\n", $lines));
}, file_get_contents($file));
file_put_contents($file, $stubs);
