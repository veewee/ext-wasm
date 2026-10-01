<?php

declare(strict_types=1);

/**
 * Works out what listens on each host:port given on the command line, with
 * the Rust component in src/lib.rs. The component may connect to exactly
 * these targets and nothing else: tcpHosts is the list itself.
 *
 *   php probe.php localhost:22 localhost:5432 localhost:6379 example.com:443
 */

use Wasm\Component\Component;
use Wasm\Component\Instance;
use Wasm\Exception\ComponentError;

$targets = array_slice($argv, 1);
if ($targets === []) {
    fwrite(STDERR, "usage: php probe.php host:port [host:port ...]\n");
    exit(2);
}

$wasi = new Wasm\Wasi(tcpHosts: $targets);
$probe = (new Instance(Component::fromFile(__DIR__ . '/service-probe.wasm'), wasi: $wasi))
    ->exports->get('example:service-probe/probe');

foreach ($targets as $target) {
    try {
        $report = $probe->identify($target, 2000);
        printf("%-28s %-11s %s (%s, connected in %d ms)\n", $target, $report['service'], $report['detail'], $report['address'], $report['connectMs']);
    } catch (ComponentError $error) {
        printf("%-28s %-11s %s\n", $target, '-', $error->payload);
    }
}
