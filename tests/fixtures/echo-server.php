<?php

// A TCP server for ComponentSocketsTest: prints its port, then answers every
// connection with "echo:" and what it received, and logs each message.
// Arguments: the address to listen on (127.0.0.1 or [::1]) and the log file.

$server = stream_socket_server("tcp://{$argv[1]}:0", $errno, $error) or exit("$error\n");
echo parse_url('tcp://' . stream_socket_get_name($server, false), PHP_URL_PORT), "\n";
while ($client = stream_socket_accept($server, -1)) {
    $message = stream_get_contents($client);
    file_put_contents($argv[2], $message . "\n", FILE_APPEND);
    fwrite($client, "echo:$message");
    fclose($client);
}
