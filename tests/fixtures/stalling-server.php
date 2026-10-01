<?php

// Accepts TCP connections on the port given as the first argument and never
// answers, for the timeout tests. Runs until it is killed.
$server = stream_socket_server('tcp://127.0.0.1:' . $argv[1]);
$clients = [];
while (true) {
    $client = @stream_socket_accept($server, 1);
    if ($client !== false) {
        $clients[] = $client;
    }
}
