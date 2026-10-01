<?php

declare(strict_types=1);

namespace Test;

require_once __DIR__ . '/AwaitsForkedChild.php';

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\RequiresPhpExtension;
use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Instance;
use Wasm\Wasi;

/**
 * UDP from tests/fixtures/component-wasi, against an echo server in a child
 * process: the guest blocks this process while it waits for a reply.
 */
final class ComponentUdpTest extends TestCase
{
    use AwaitsForkedChild;

    private const FIXTURE = __DIR__ . '/fixtures/component-wasi/component-wasi.wasm';

    private static ?Component $component = null;

    /** @var list<array{resource, string}> running servers and their logs */
    private array $servers = [];

    protected function tearDown(): void
    {
        foreach ($this->servers as [$process, $log]) {
            proc_terminate($process);
            proc_close($process);
            @unlink($log);
        }
    }

    /**
     * @param list<string>|null $udpHosts
     * @param list<string>|null $tcpHosts
     */
    private static function guest(?array $udpHosts, string $mode, string $address, string $message, ?array $tcpHosts = null): string
    {
        $wasi = new Wasi(args: ['app', $mode, $address, $message], udpHosts: $udpHosts, tcpHosts: $tcpHosts);
        $wasi->start(new Instance(self::$component ??= Component::fromFile(self::FIXTURE), wasi: $wasi));

        return $wasi->stdout();
    }

    /** @return array{int, string} the port and the log of datagrams */
    private function echoServer(bool $replyFromOtherPort = false): array
    {
        $log = tempnam(sys_get_temp_dir(), 'wasm-udp');
        $arguments = [PHP_BINARY, '-n', __DIR__ . '/fixtures/udp-echo-server.php', '127.0.0.1', $log];
        if ($replyFromOtherPort) {
            $arguments[] = 'other';
        }
        $process = proc_open($arguments, [1 => ['pipe', 'w']], $pipes);
        $port = trim((string) fgets($pipes[1]));
        if (!ctype_digit($port)) {
            proc_terminate($process);
            proc_close($process);
            @unlink($log);
            self::markTestSkipped("cannot listen on UDP: $port");
        }
        $this->servers[] = [$process, $log];

        return [(int) $port, $log];
    }

    /** What reached the server before a datagram this test sends now: UDP has no other way to tell "nothing" from "not yet". */
    private static function receivedBefore(int $port, string $log): string
    {
        $client = stream_socket_client("udp://127.0.0.1:$port");
        fwrite($client, 'sentinel');
        $deadline = microtime(true) + 5;
        while (!str_contains((string) file_get_contents($log), "sentinel\n") && microtime(true) < $deadline) {
            usleep(10_000);
        }

        return str_replace("sentinel\n", '', (string) file_get_contents($log));
    }

    public function test_without_udp_hosts_nothing_is_sent(): void
    {
        [$port, $log] = $this->echoServer();

        self::assertStringStartsWith('error: PermissionDenied', self::guest(null, 'udp', "127.0.0.1:$port", 'hi'));
        self::assertSame('', self::receivedBefore($port, $log));
    }

    public function test_an_address_rule_sends_and_receives(): void
    {
        [$port, $log] = $this->echoServer();

        self::assertSame('reply: echo:hi', self::guest(["127.0.0.1:$port"], 'udp', "127.0.0.1:$port", 'hi'));
        self::assertSame("hi\n", self::receivedBefore($port, $log));
    }

    public function test_a_name_rule_sends_and_receives(): void
    {
        [$port] = $this->echoServer();

        self::assertSame('reply: echo:by name', self::guest(["localhost:$port"], 'udp', "localhost:$port", 'by name'));
    }

    /** @return iterable<string, array{\Closure(int): list<string>, \Closure(int): string}> */
    public static function refusedDestinations(): iterable
    {
        yield 'another port' => [static fn (int $port): array => ["127.0.0.1:$port"], static fn (int $port): string => '127.0.0.1:' . ($port === 1 ? 2 : $port - 1)];
        yield 'another address on a name rule port' => [static fn (int $port): array => ["localhost:$port"], static fn (int $port): string => "127.0.0.2:$port"];
    }

    /**
     * @param \Closure(int): list<string> $rules
     * @param \Closure(int): string $destination
     */
    #[DataProvider('refusedDestinations')]
    public function test_a_send_no_rule_allows_is_refused_at_once(\Closure $rules, \Closure $destination): void
    {
        [$port, $log] = $this->echoServer();

        self::assertStringStartsWith('error: PermissionDenied', self::guest($rules($port), 'udp', $destination($port), 'hi'));
        self::assertSame('', self::receivedBefore($port, $log));
    }

    public function test_a_reply_from_an_address_no_rule_allows_is_dropped(): void
    {
        [$port, $log] = $this->echoServer(replyFromOtherPort: true);

        self::assertSame('error: TimedOut: no reply', self::guest(["127.0.0.1:$port"], 'udp', "127.0.0.1:$port", 'hi'));
        self::assertSame("hi\n", self::receivedBefore($port, $log));
    }

    public function test_tcp_and_udp_rules_stay_apart(): void
    {
        [$port] = $this->echoServer();

        self::assertStringStartsWith('error: PermissionDenied', self::guest(null, 'udp', "127.0.0.1:$port", 'hi', tcpHosts: ["127.0.0.1:$port"]));
        self::assertStringStartsWith('error: PermissionDenied', self::guest(["127.0.0.1:$port"], 'tcp', "127.0.0.1:$port", 'hi'));
    }

    public function test_invalid_rules_name_udp_hosts(): void
    {
        $this->expectException(\ValueError::class);
        $this->expectExceptionMessage('udpHosts entry "statsd" needs a port');

        new Wasi(udpHosts: ['statsd']);
    }

    public function test_non_string_rules_are_a_type_error(): void
    {
        $this->expectException(\TypeError::class);
        $this->expectExceptionMessage('udpHosts entries must be strings, got int');

        new Wasi(udpHosts: [8125]);
    }

    #[RequiresPhpExtension('pcntl')]
    public function test_a_forked_child_sends_after_the_parent_did(): void
    {
        [$port] = $this->echoServer();
        self::assertSame('reply: echo:parent', self::guest(["localhost:$port"], 'udp', "localhost:$port", 'parent'));

        $pid = pcntl_fork();
        if ($pid === 0) {
            exit(self::guest(["localhost:$port"], 'udp', "localhost:$port", 'child') === 'reply: echo:child' ? 0 : 1);
        }

        $this->assertChildExitsCleanly($pid, 'The forked child did not send');
    }
}
