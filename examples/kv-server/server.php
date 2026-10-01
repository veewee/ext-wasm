<?php

// A Redis compatible key-value server on 127.0.0.1, port 6380 or the first argument:
//   php examples/kv-server/server.php [port]
//   redis-cli -p 6380 set greeting hello
// PHP accepts the connections and the component speaks the protocol, one connection at a time.

declare(strict_types=1);

require __DIR__ . '/KvServer.php';

$port = (int) ($argv[1] ?? 6380);
$server = stream_socket_server("tcp://127.0.0.1:$port", $code, $message)
    ?: exit("cannot listen on port $port: $message\n");
$kv = new KvServer();
echo "listening on 127.0.0.1:$port\n";

while (true) {
    $client = @stream_socket_accept($server, -1);
    if ($client === false) {
        continue;
    }
    $kv->serve($client);
    fclose($client);
}
