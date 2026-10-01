<?php

// Completes the stubs cargo-php generates, with the extension they were
// generated from loaded, so reflection fills in what cargo-php cannot know:
//
// - cargo-php writes every class as plain "class", because the class
//   description ext-php-rs gives it carries no flags. Reflection says which
//   classes are final, abstract or readonly.
// - cargo-php gives nullable readonly properties a "= null" default, which PHP
//   refuses for readonly properties.
// - cargo-php appends its own "@param" and "@return" tags to every docblock,
//   also when the doc comment already documents a more precise type. The
//   first tag, written in the doc comment, is kept.

declare(strict_types=1);

if (!extension_loaded('wasm') || !isset($argv[1])) {
    fwrite(STDERR, "usage: php -d extension=<the extension the stubs come from> {$argv[0]} <stubs>\n");
    exit(1);
}

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

$namespace = '';
$lines = [];
foreach (explode("\n", $stubs) as $line) {
    if (preg_match('~^namespace ([\w\\\\]+) \{$~', $line, $match)) {
        $namespace = $match[1];
    } elseif (preg_match('~^(\s*)class (\w+)~', $line, $match)) {
        $class = new ReflectionClass($namespace . '\\' . $match[2]);
        $modifiers = ($class->isFinal() ? 'final ' : '')
            . ($class->isAbstract() ? 'abstract ' : '')
            . ($class->isReadOnly() ? 'readonly ' : '');
        $line = $match[1] . $modifiers . substr($line, strlen($match[1]));
    } elseif (preg_match('~^\s*public readonly .* = null;$~', $line)) {
        $line = substr($line, 0, -strlen(' = null;')) . ';';
    }
    $lines[] = $line;
}
file_put_contents($file, implode("\n", $lines));
