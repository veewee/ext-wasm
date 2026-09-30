<?php

// Creates and drops instances in a loop and reports time and peak memory.
// Usage: php -d extension=... tools/bench-instances.php [count] [--keep]
// --keep holds one instance for the whole run, as a long-running worker would.

declare(strict_types=1);

$count = (int) ($argv[1] ?? 10000);
$keep = in_array('--keep', $argv, true);
$module = new Wasm\Module('(module (memory (export "memory") 1) (func (export "run") (result i32) i32.const 1))');
$kept = $keep ? new Wasm\Instance($module) : null;

$start = hrtime(true);
for ($i = 0; $i < $count; $i++) {
    $instance = new Wasm\Instance($module);
    $instance->exports->memory->write(0, str_repeat("\1", 65536));
    $instance->exports->run();
    unset($instance);
}
$elapsed = (hrtime(true) - $start) / 1e6;

// ru_maxrss is in bytes on macOS and in KiB on Linux.
$peak = getrusage()['ru_maxrss'] / (PHP_OS_FAMILY === 'Darwin' ? 1048576 : 1024);
printf("%d instances%s: %.1f ms, peak RSS %.1f MiB\n", $count, $keep ? ' (one kept)' : '', $elapsed, $peak);
