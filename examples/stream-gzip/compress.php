<?php

// Compresses a file of any size with gzip, chunk by chunk.
// Usage: php examples/stream-gzip/compress.php input [output.gz]
// Without an input it compresses 50 MB of generated log lines.

declare(strict_types=1);

require __DIR__ . '/Gzip.php';

$input = $argv[1] ?? null;
$output = $argv[2] ?? ($input !== null ? "$input.gz" : sys_get_temp_dir() . '/stream-gzip-sample.log.gz');

$chunks = $input !== null ? Gzip::read($input) : (function () {
    // A thousand lines per chunk: every chunk is one round trip to PHP.
    for ($i = 0; $i < 500_000; $i += 1000) {
        $lines = '';
        for ($j = $i; $j < $i + 1000; ++$j) {
            $lines .= sprintf("%s INFO request %d served in %d ms\n", date('c', 1_700_000_000 + $j), $j, $j % 250) . str_repeat('.', 50) . "\n";
        }
        yield $lines;
    }
})();

$start = hrtime(true);
$in = $out = 0;
$target = fopen($output, 'wb');
foreach ((new Gzip())->compress((function () use ($chunks, &$in) {
    foreach ($chunks as $chunk) {
        $in += strlen($chunk);
        yield $chunk;
    }
})()) as $compressed) {
    $out += strlen($compressed);
    fwrite($target, $compressed);
}
fclose($target);

printf(
    "%s: %.1f MB -> %.1f MB in %.0f ms, peak PHP memory %.1f MB\n",
    $output,
    $in / 1e6,
    $out / 1e6,
    (hrtime(true) - $start) / 1e6,
    memory_get_peak_usage() / 1e6,
);
