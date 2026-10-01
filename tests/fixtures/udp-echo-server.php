<?php

// A UDP server for ComponentUdpTest: prints its port, then answers every
// datagram with "echo:" and the datagram, and logs each one. Arguments: the
// address to listen on and the log file; with a third argument "other", it
// answers from a second socket, at another port.

$server = stream_socket_server("udp://{$argv[1]}:0", $errno, $error, STREAM_SERVER_BIND) or exit("$error\n");
$reply = ($argv[3] ?? '') === 'other'
    ? stream_socket_server("udp://{$argv[1]}:0", $errno, $error, STREAM_SERVER_BIND)
    : $server;
echo parse_url('udp://' . stream_socket_get_name($server, false), PHP_URL_PORT), "\n";
while (true) {
    $message = stream_socket_recvfrom($server, 512, 0, $peer);
    file_put_contents($argv[2], $message . "\n", FILE_APPEND);
    stream_socket_sendto($reply, "echo:$message", 0, $peer);
}
