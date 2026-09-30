<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\Attributes\RequiresPhpExtension;
use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Instance;
use Wasm\Exception\LinkError;
use Wasm\Wasi;

require_once __DIR__ . '/AwaitsForkedChild.php';

/**
 * tests/fixtures/component-http is a Rust command that GETs the URL it is
 * given. The tests serve requests with PHP's built-in server on 127.0.0.1.
 */
final class ComponentHttpTest extends TestCase
{
    use AwaitsForkedChild;

    private const FIXTURE = __DIR__ . '/fixtures/component-http/component-http.wasm';

    /** @var resource|null */
    private static $server = null;
    private static int $port = 0;
    private static string $log = '';

    public static function setUpBeforeClass(): void
    {
        $probe = stream_socket_server('tcp://127.0.0.1:0');
        self::$port = (int) substr(strrchr(stream_socket_get_name($probe, false), ':'), 1);
        fclose($probe);
        self::$log = tempnam(sys_get_temp_dir(), 'wasm-http-log');
        self::$server = proc_open(
            [PHP_BINARY, '-n', '-S', '127.0.0.1:' . self::$port, __DIR__ . '/fixtures/http-server.php'],
            [0 => ['pipe', 'r'], 1 => ['file', self::nullDevice(), 'w'], 2 => ['file', self::nullDevice(), 'w']],
            $pipes,
            null,
            ['HTTP_TEST_LOG' => self::$log],
        );
        $deadline = microtime(true) + 10;
        while (@fsockopen('127.0.0.1', self::$port) === false) {
            if (microtime(true) > $deadline) {
                self::fail('the test HTTP server did not start');
            }
            usleep(50_000);
        }
    }

    private static function nullDevice(): string
    {
        return PHP_OS_FAMILY === 'Windows' ? 'nul' : '/dev/null';
    }

    public static function tearDownAfterClass(): void
    {
        if (self::$server !== null) {
            proc_terminate(self::$server);
            proc_close(self::$server);
        }
    }

    protected function setUp(): void
    {
        file_put_contents(self::$log, '');
    }

    /** @param list<string>|null $hosts */
    private static function get(string $url, ?array $hosts): string
    {
        $wasi = $hosts === null ? new Wasi(args: ['app', 'get', $url]) : new Wasi(args: ['app', 'get', $url], httpHosts: $hosts);
        $wasi->start(new Instance(Component::fromFile(self::FIXTURE), wasi: $wasi));

        return $wasi->stdout();
    }

    private static function url(string $path = '/'): string
    {
        return 'http://127.0.0.1:' . self::$port . $path;
    }

    public function test_an_allowed_host_is_reached(): void
    {
        self::assertSame("200\nhello from /greeting", self::get(self::url('/greeting'), ['127.0.0.1']));
        self::assertSame("GET /greeting\n", file_get_contents(self::$log));
    }

    public function test_a_host_that_is_not_allowed_is_denied_before_sending(): void
    {
        self::assertSame('error: ErrorCode::HttpRequestDenied', self::get(self::url(), ['example.com']));
        self::assertSame('', file_get_contents(self::$log));
    }

    public function test_an_empty_list_denies_every_request(): void
    {
        self::assertSame('error: ErrorCode::HttpRequestDenied', self::get(self::url(), []));
    }

    public function test_a_host_with_a_port_allows_only_that_port(): void
    {
        self::assertSame("200\nhello from /", self::get(self::url(), ['127.0.0.1:' . self::$port]));
        self::assertSame('error: ErrorCode::HttpRequestDenied', self::get(self::url(), ['127.0.0.1:1']));
    }

    public function test_a_wildcard_allows_subdomains_but_not_the_domain_itself(): void
    {
        // .invalid never resolves, so an allowed request fails on the name lookup instead.
        self::assertStringNotContainsString('HttpRequestDenied', self::get('http://api.example.invalid/', ['*.example.invalid']));
        self::assertSame('error: ErrorCode::HttpRequestDenied', self::get('http://example.invalid/', ['*.example.invalid']));
        self::assertSame('error: ErrorCode::HttpRequestDenied', self::get('http://evilexample.invalid/', ['*.example.invalid']));
    }

    public function test_user_info_in_the_url_does_not_bypass_the_list(): void
    {
        self::assertSame('error: ErrorCode::HttpRequestDenied', self::get('http://127.0.0.1@example.invalid/', ['127.0.0.1']));
        self::assertSame('', file_get_contents(self::$log));
    }

    public function test_requests_time_out_after_default_socket_timeout(): void
    {
        $previous = ini_set('default_socket_timeout', '1');
        try {
            $started = microtime(true);
            $output = self::get(self::url('/slow?sleep=3'), ['127.0.0.1']);
        } finally {
            ini_set('default_socket_timeout', (string) $previous);
        }

        self::assertStringStartsWith('error: ', $output);
        self::assertLessThan(2.5, microtime(true) - $started);
    }

    public function test_without_http_hosts_wasi_http_is_not_linked(): void
    {
        $this->expectException(LinkError::class);
        $this->expectExceptionMessage('wasi:http');
        self::get(self::url(), null);
    }

    public function test_an_entry_must_be_a_string(): void
    {
        $this->expectException(\TypeError::class);
        new Wasi(httpHosts: [80]);
    }

    public function test_an_entry_must_not_have_a_scheme_or_path(): void
    {
        foreach (['http://example.com', 'example.com/api', '', 'bücher.example', 'example.com:http'] as $entry) {
            try {
                new Wasi(httpHosts: [$entry]);
                self::fail("Expected a ValueError for \"$entry\"");
            } catch (\ValueError $error) {
                self::assertStringContainsString('httpHosts', $error->getMessage());
            }
        }
    }

    #[RequiresPhpExtension('pcntl')]
    public function test_a_forked_child_makes_requests_after_the_parent_did(): void
    {
        self::assertSame("200\nhello from /parent", self::get(self::url('/parent'), ['127.0.0.1']));

        $pid = pcntl_fork();
        if ($pid === 0) {
            exit(self::get(self::url('/child'), ['127.0.0.1']) === "200\nhello from /child" ? 0 : 1);
        }

        $this->assertChildExitsCleanly($pid, 'The forked child did not finish its HTTP request');
    }
}
