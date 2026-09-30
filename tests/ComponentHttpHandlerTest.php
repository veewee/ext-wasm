<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Http\Request;
use Wasm\Component\Http\Response;
use Wasm\Component\Instance;
use Wasm\Exception\RuntimeError;
use Wasm\Suspending;
use Wasm\Wasi;

/**
 * tests/fixtures/component-http-handler is a wasi:http/proxy component that
 * echoes the request; see its src/lib.rs for the special paths.
 */
final class ComponentHttpHandlerTest extends TestCase
{
    private const HANDLER = __DIR__ . '/fixtures/component-http-handler/component-http-handler.wasm';

    private static ?Component $component = null;

    private static function handler(): Instance
    {
        self::$component ??= Component::fromFile(self::HANDLER);

        return new Instance(self::$component, wasi: new Wasi(httpHosts: []));
    }

    /** A handler whose answer is what `later(1)` returns. */
    private static function asyncHandler(Suspending $later): Instance
    {
        return new Instance(
            Component::fromFile(__DIR__ . '/fixtures/component-http-handler-async/component-http-handler-async.wasm'),
            ['later' => $later],
            new Wasi(httpHosts: []),
        );
    }

    public function test_an_async_instance_answers_after_its_fiber_resumes(): void
    {
        $handler = self::asyncHandler(new Suspending(fn (int $n): int => \Fiber::suspend('waiting') + $n));

        $fiber = new \Fiber(fn (): Response => $handler->handle(new Request('GET', 'http://localhost/')));
        self::assertSame('waiting', $fiber->start());
        $fiber->resume(41);

        self::assertSame('42', $fiber->getReturn()->body);
    }

    public function test_an_async_instance_answers_without_suspending(): void
    {
        $handler = self::asyncHandler(new Suspending(fn (int $n): int => $n * 10));

        self::assertSame('10', $handler->handle(new Request('GET', 'http://localhost/'))->body);
    }

    public function test_a_parked_async_instance_refuses_another_request(): void
    {
        $handler = self::asyncHandler(new Suspending(fn (int $n): int => \Fiber::suspend() + $n));
        $fiber = new \Fiber(fn (): Response => $handler->handle(new Request('GET', 'http://localhost/')));
        $fiber->start();

        try {
            $handler->handle(new Request('GET', 'http://localhost/'));
            self::fail('a parked instance took another request');
        } catch (RuntimeError $e) {
            self::assertStringContainsString('the store is busy with a suspended call', $e->getMessage());
        }
        $fiber->resume(1);
        self::assertSame('2', $fiber->getReturn()->body);
    }

    public function test_the_component_answers_a_request(): void
    {
        $response = self::handler()->handle(new Request('GET', 'http://localhost/hello?name=ada', ['X-Greeting' => 'hi']));

        self::assertInstanceOf(Response::class, $response);
        self::assertSame(201, $response->status);
        self::assertSame(['Method::Get /hello?name=ada'], $response->headers['x-echo']);
        self::assertSame(['hi'], $response->headers['x-greeting']);
        self::assertSame('', $response->body);
    }

    public function test_the_request_body_reaches_the_component(): void
    {
        $response = self::handler()->handle(new Request('POST', 'http://localhost/echo', [], "binary\x00body"));

        self::assertSame(['Method::Post /echo'], $response->headers['x-echo']);
        self::assertSame("binary\x00body", $response->body);
    }

    public function test_a_large_response_body_is_collected_while_it_is_written(): void
    {
        $response = self::handler()->handle(new Request('GET', 'http://localhost/large'));

        self::assertSame(3 * 1024 * 1024, strlen($response->body));
    }

    public function test_one_instance_handles_several_requests(): void
    {
        $handler = self::handler();

        self::assertSame('one', $handler->handle(new Request('POST', 'http://localhost/', [], 'one'))->body);
        self::assertSame('two', $handler->handle(new Request('POST', 'http://localhost/', [], 'two'))->body);
    }

    public function test_an_error_code_from_the_component_is_a_runtime_error(): void
    {
        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('HttpRequestDenied');
        self::handler()->handle(new Request('GET', 'http://localhost/error'));
    }

    public function test_a_component_that_never_answers_is_a_runtime_error(): void
    {
        $this->expectException(RuntimeError::class);
        $this->expectExceptionMessage('did not set a response');
        self::handler()->handle(new Request('GET', 'http://localhost/silent'));
    }

    public function test_a_component_without_an_incoming_handler_cannot_handle_requests(): void
    {
        $instance = new Instance(new Component('(component)'));

        $this->expectException(\Error::class);
        $this->expectExceptionMessage('wasi:http/incoming-handler');
        $instance->handle(new Request('GET', 'http://localhost/'));
    }

    public function test_a_request_normalises_its_headers(): void
    {
        $request = new Request('get', 'https://example.com/a', ['Accept' => ['text/html', 'text/plain'], 'X-One' => 'single']);

        self::assertSame('GET', $request->method);
        self::assertSame('https://example.com/a', $request->url);
        self::assertSame(['accept' => ['text/html', 'text/plain'], 'x-one' => ['single']], $request->headers);
        self::assertSame('', $request->body);
    }

    public function test_a_request_needs_an_absolute_http_url(): void
    {
        foreach (['/relative', 'ftp://example.com/', 'not a url'] as $url) {
            try {
                new Request('GET', $url);
                self::fail("Expected a ValueError for $url");
            } catch (\ValueError) {
                self::addToAssertionCount(1);
            }
        }
    }

    public function test_request_and_response_are_read_only_and_compare_by_value(): void
    {
        self::assertEquals(new Request('GET', 'http://localhost/'), new Request('GET', 'http://localhost/'));
        self::assertNotEquals(new Request('GET', 'http://localhost/'), new Request('GET', 'http://localhost/other'));
        self::assertEquals(new Response(200, ['a' => ['b']], 'x'), new Response(200, ['a' => 'b'], 'x'));

        $this->expectException(\Exception::class);
        $request = new Request('GET', 'http://localhost/');
        $request->method = 'POST';
    }
}
