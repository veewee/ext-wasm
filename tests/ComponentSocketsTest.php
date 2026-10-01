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
 * Outgoing TCP from tests/fixtures/component-wasi, against an echo server in a
 * child process: the guest blocks this process while it connects.
 */
final class ComponentSocketsTest extends TestCase
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

    /** @param list<string>|null $tcpHosts */
    private static function guest(?array $tcpHosts, string ...$args): string
    {
        $wasi = new Wasi(args: ['app', ...$args], tcpHosts: $tcpHosts);
        $wasi->start(new Instance(self::$component ??= Component::fromFile(self::FIXTURE), wasi: $wasi));

        return $wasi->stdout();
    }

    /** @return array{int, string} the port and the log of connections */
    private function echoServer(string $host = '127.0.0.1'): array
    {
        $log = tempnam(sys_get_temp_dir(), 'wasm-echo');
        $process = proc_open([PHP_BINARY, '-n', __DIR__ . '/fixtures/echo-server.php', $host, $log], [1 => ['pipe', 'w']], $pipes);
        $port = trim((string) fgets($pipes[1]));
        if (!ctype_digit($port)) {
            proc_terminate($process);
            proc_close($process);
            @unlink($log);
            self::markTestSkipped("cannot listen on $host: $port");
        }
        $this->servers[] = [$process, $log];

        return [(int) $port, $log];
    }

    public function test_without_tcp_hosts_a_connect_is_refused_and_nothing_arrives(): void
    {
        [$port, $log] = $this->echoServer();

        self::assertStringStartsWith('error: PermissionDenied', self::guest(null, 'tcp', "127.0.0.1:$port", 'hi'));
        self::assertSame('', file_get_contents($log));
    }

    public function test_an_empty_list_refuses_every_connect(): void
    {
        [$port] = $this->echoServer();

        self::assertStringStartsWith('error: PermissionDenied', self::guest([], 'tcp', "127.0.0.1:$port", 'hi'));
    }

    public function test_an_allowed_address_is_reached(): void
    {
        [$port, $log] = $this->echoServer();

        self::assertSame('reply: echo:hi', self::guest(["127.0.0.1:$port"], 'tcp', "127.0.0.1:$port", 'hi'));
        self::assertSame("hi\n", file_get_contents($log));
    }

    public function test_a_name_rule_lets_the_guest_resolve_and_connect(): void
    {
        [$port] = $this->echoServer();

        self::assertSame('reply: echo:by name', self::guest(["localhost:$port"], 'tcp', "localhost:$port", 'by name'));
    }

    public function test_a_name_rule_allows_the_addresses_it_resolves_to(): void
    {
        [$port] = $this->echoServer();

        self::assertSame('reply: echo:literal', self::guest(["localhost:$port"], 'tcp', "127.0.0.1:$port", 'literal'));
    }

    public function test_without_a_name_rule_the_guest_cannot_look_up_names(): void
    {
        [$port] = $this->echoServer();

        self::assertStringContainsString('failed to lookup address information', self::guest(["127.0.0.1:$port"], 'tcp', "localhost:$port", 'hi'));
    }

    /** @return iterable<string, array{\Closure(int): list<string>}> */
    public static function rulesThatDoNotMatch(): iterable
    {
        yield 'another port' => [static fn (int $port): array => ['127.0.0.1:' . ($port === 1 ? 2 : $port - 1)]];
        yield 'another address' => [static fn (int $port): array => ["127.0.0.2:$port"]];
        yield 'a network without it' => [static fn (int $port): array => ["10.0.0.0/8:$port"]];
    }

    /** @param \Closure(int): list<string> $rules */
    #[DataProvider('rulesThatDoNotMatch')]
    public function test_a_destination_no_rule_matches_is_refused(\Closure $rules): void
    {
        [$port, $log] = $this->echoServer();

        self::assertStringStartsWith('error: PermissionDenied', self::guest($rules($port), 'tcp', "127.0.0.1:$port", 'hi'));
        self::assertSame('', file_get_contents($log));
    }

    public function test_an_ipv4_mapped_network_matches_as_its_ipv4_network(): void
    {
        [$port] = $this->echoServer();

        self::assertSame('reply: echo:mapped', self::guest(["[::ffff:127.0.0.0/104]:$port"], 'tcp', "127.0.0.1:$port", 'mapped'));
        self::assertStringStartsWith('error: PermissionDenied', self::guest(["[::ffff:10.0.0.0/104]:$port"], 'tcp', "127.0.0.1:$port", 'hi'));
    }

    public function test_a_network_with_any_port_matches(): void
    {
        [$port] = $this->echoServer();

        self::assertSame('reply: echo:hi', self::guest(['127.0.0.0/8:*'], 'tcp', "127.0.0.1:$port", 'hi'));
    }

    public function test_ipv6_rules_match_ipv6_addresses(): void
    {
        [$port] = $this->echoServer('[::1]');

        self::assertSame('reply: echo:six', self::guest(["[::1]:$port"], 'tcp', "[::1]:$port", 'six'));
        self::assertSame('reply: echo:six', self::guest(["[::1/128]:$port"], 'tcp', "[::1]:$port", 'six'));
    }

    public function test_listening_stays_refused(): void
    {
        $rules = ['127.0.0.0/8:*'];

        // A specific address is refused at bind, the wildcard one at listen,
        // since its bind looks the same as the one every connect makes.
        self::assertStringStartsWith('error: PermissionDenied', self::guest($rules, 'listen', '127.0.0.1:0'));
        self::assertStringStartsWith('error: PermissionDenied', self::guest($rules, 'listen', '0.0.0.0:0'));
    }

    /** @return iterable<string, array{mixed, string, string}> */
    public static function invalidRules(): iterable
    {
        yield 'no port' => ['db.internal', \ValueError::class, 'tcpHosts entry "db.internal" needs a port'];
        yield 'port out of range' => ['db:99999', \ValueError::class, 'tcpHosts entry "db:99999" has an invalid port'];
        yield 'port zero' => ['db:0', \ValueError::class, 'tcpHosts entry "db:0" has an invalid port'];
        yield 'wildcard name' => ['*.example.com:80', \ValueError::class, 'tcpHosts entry "*.example.com:80"'];
        yield 'prefix too long' => ['10.0.0.0/33:1', \ValueError::class, 'tcpHosts entry "10.0.0.0/33:1"'];
        yield 'name with prefix' => ['db/8:1', \ValueError::class, 'tcpHosts entry "db/8:1"'];
        yield 'number libc reads as an address' => ['10.0.1:6379', \ValueError::class, 'tcpHosts entry "10.0.1:6379"'];
        yield 'mapped network too wide' => ['[::ffff:0:0/64]:1', \ValueError::class, 'below 96 for an IPv4-mapped address'];
        yield 'not a string' => [5432, \TypeError::class, 'tcpHosts entries must be strings, got int'];
    }

    #[DataProvider('invalidRules')]
    public function test_invalid_rules_are_rejected(mixed $rule, string $class, string $message): void
    {
        $this->expectException($class);
        $this->expectExceptionMessage($message);

        new Wasi(tcpHosts: [$rule]);
    }

    #[RequiresPhpExtension('pcntl')]
    public function test_a_forked_child_connects_after_the_parent_did(): void
    {
        [$port] = $this->echoServer();
        self::assertSame('reply: echo:parent', self::guest(["localhost:$port"], 'tcp', "localhost:$port", 'parent'));

        $pid = pcntl_fork();
        if ($pid === 0) {
            exit(self::guest(["localhost:$port"], 'tcp', "localhost:$port", 'child') === 'reply: echo:child' ? 0 : 1);
        }

        $this->assertChildExitsCleanly($pid, 'The forked child did not connect');
    }
}
