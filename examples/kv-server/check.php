<?php

// Talks to a running server.php with a small RESP client and checks the replies.
// Usage: php examples/kv-server/check.php [port]
// It uses keys starting with "check:" and leaves them behind.

declare(strict_types=1);

$port = (int) ($argv[1] ?? 6380);

function connect(int $port)
{
    $connection = stream_socket_client("tcp://127.0.0.1:$port", $code, $message, 2)
        ?: throw new RuntimeException("cannot connect to port $port: $message");
    stream_set_timeout($connection, 2);

    return $connection;
}

/** @param list<string> $args */
function command(array $args): string
{
    $out = '*' . count($args) . "\r\n";
    foreach ($args as $arg) {
        $out .= '$' . strlen($arg) . "\r\n$arg\r\n";
    }

    return $out;
}

function reply($connection): mixed
{
    $line = fgets($connection);
    if ($line === false) {
        throw new RuntimeException(stream_get_meta_data($connection)['timed_out'] ? 'no reply within 2 seconds' : 'connection closed');
    }
    $line = substr($line, 0, -2);
    $rest = substr($line, 1);

    return match ($line[0]) {
        '+' => $rest,
        '-' => "error: $rest",
        ':' => (int) $rest,
        '$' => $rest === '-1' ? null : substr(stream_get_contents($connection, (int) $rest + 2) ?: '', 0, -2),
        '*' => (int) $rest < 1 ? [] : array_map(fn () => reply($connection), range(1, (int) $rest)),
    };
}

$passed = 0;
function expect(string $label, mixed $actual, mixed $expected): void
{
    global $passed;
    if ($actual !== $expected) {
        fwrite(STDERR, sprintf("FAIL %s: expected %s, got %s\n", $label, json_encode($expected), json_encode($actual)));
        exit(1);
    }
    printf("ok   %-40s %s\n", $label, json_encode($actual));
    ++$passed;
}

/** @param list<string> $args */
function call($connection, array $args): mixed
{
    fwrite($connection, command($args));

    return reply($connection);
}

$c = connect($port);
call($c, ['DEL', 'check:a', 'check:b', 'check:c', 'check:n', 'check:t']);
expect('PING', call($c, ['PING']), 'PONG');
expect('SET check:a 1', call($c, ['SET', 'check:a', '1']), 'OK');
expect('GET check:a', call($c, ['GET', 'check:a']), '1');
expect('GET of a missing key', call($c, ['GET', 'check:missing']), null);

fwrite($c, command(['INCR', 'check:n']) . command(['INCR', 'check:n']) . command(['INCR', 'check:n']));
expect('three pipelined INCR', [reply($c), reply($c), reply($c)], [1, 2, 3]);

$split = command(['SET', 'check:b', "two\r\nlines"]);
fwrite($c, substr($split, 0, 20));
usleep(100_000);
fwrite($c, substr($split, 20));
expect('SET split over two writes', reply($c), 'OK');
expect('GET check:b', call($c, ['GET', 'check:b']), "two\r\nlines");

expect('EXISTS of two out of three', call($c, ['EXISTS', 'check:a', 'check:b', 'check:missing']), 2);
$keys = call($c, ['KEYS', 'check:?']);
sort($keys);
expect('KEYS check:?', $keys, ['check:a', 'check:b', 'check:n']);
expect('INCR of a non-integer', call($c, ['INCR', 'check:b']), 'error: ERR value is not an integer or out of range');

expect('SET with EX 1', call($c, ['SET', 'check:t', 'soon gone', 'EX', '1']), 'OK');
expect('GET before it expires', call($c, ['GET', 'check:t']), 'soon gone');
usleep(1_100_000);
expect('GET after it expired', call($c, ['GET', 'check:t']), null);

fwrite($c, "PING hello\r\n");
expect('inline PING hello', reply($c), 'hello');
expect('unknown command', call($c, ['FLY']), "error: ERR unknown command 'FLY'");
expect('GET without a key', call($c, ['GET']), "error: ERR wrong number of arguments for 'get' command");
expect('DEL check:b', call($c, ['DEL', 'check:b']), 1);
expect('QUIT', call($c, ['QUIT']), 'OK');
expect('connection closed after QUIT', fread($c, 1), '');
fclose($c);

$c = connect($port);
fwrite($c, command(['SET', 'check:c', 'sent, reply unread']));
fclose($c);

$c = connect($port);
expect('GET check:a on a new connection', call($c, ['GET', 'check:a']), '1');
expect('GET of a key set by a client that left', call($c, ['GET', 'check:c']), 'sent, reply unread');
fclose($c);

echo "$passed checks passed\n";
