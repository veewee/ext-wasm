<?php

// Prints the last commits of a repository, like git log --oneline.
// Usage: php examples/git/log.php /path/to/repo [revision] [count]

declare(strict_types=1);

require __DIR__ . '/Git.php';

$path = $argv[1] ?? '.';
$revision = $argv[2] ?? 'HEAD';
$count = (int) ($argv[3] ?? 10);
$realPath = realpath($path);
if ($realPath === false) {
    fwrite(STDERR, "usage: php log.php /path/to/repo [revision] [count]\n");
    exit(2);
}

try {
    $git = new Git($realPath);
    foreach ($git->log($revision, $count) as $commit) {
        printf("%s %s  (%s, %s)\n", substr($commit['hash'], 0, 7), strtok($commit['message'], "\n"), $commit['author']['name'], date('Y-m-d', $commit['author']['time']));
    }
} catch (Wasm\Exception\ComponentError $e) {
    fwrite(STDERR, "failed: {$e->payload}\n");
    exit(1);
}
