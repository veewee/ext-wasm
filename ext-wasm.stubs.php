<?php

// Stubs for wasm

namespace Wasm {
    /**
     * The exports of an instance, like JS `instance.exports`.
     *
     * Wrapper objects are created once, so `$exports->f === $exports->f` holds as in JS.
     */
    final class Exports implements \Iterator {
        /**
         * @param string $name
         * @param array $arguments
         * @return mixed
         */
        public function __call(string $name, array $arguments): mixed {}

        public function __construct() {}

        /**
         * @param string $name
         * @return mixed
         */
        public function __get(string $name): mixed {}

        /**
         * @param string $name
         * @return bool
         */
        public function __isset(string $name): bool {}

        /**
         * @return mixed
         */
        public function current(): mixed {}

        /**
         * @return string|null
         */
        public function key(): ?string {}

        /**
         * @return void
         */
        public function next(): void {}

        /**
         * @return void
         */
        public function rewind(): void {}

        /**
         * @return bool
         */
        public function valid(): bool {}
    }

    /**
     * A wasm function: one a module exports, or a PHP callable of a given
     * function type, like JS `new WebAssembly.Function(type, fn)`.
     */
    final class Func {
        /**
         * @param array{parameters: list<string>, results: list<string>} $type
         * @param callable $callback
         */
        public function __construct(array $type, mixed $callback) {}

        /**
         * @param mixed $args
         * @return mixed
         */
        public function __invoke(mixed ...$args): mixed {}

        /**
         * Number of parameters, like JS `Function.prototype.length`.
         *
         * @return int
         */
        public function length(): int {}

        /**
         * @return array{parameters: list<string>, results: list<string>}
         */
        public function type(): array {}
    }

    /**
     * A wasm global, like JS `WebAssembly.Global`. Named GlobalVar because
     * `Global` is a reserved word in PHP.
     *
     * @property mixed $value
     */
    final class GlobalVar {
        /**
         * `value` is a value type name as `type()` gives it; reference types other
         * than func and extern ones cannot hold a PHP value.
         *
         * @param array{value: string, mutable?: bool} $descriptor
         *
         * @param mixed $value
         * @param \Wasm\Store|null $store
         */
        public function __construct(array $descriptor, mixed $value = null, ?\Wasm\Store $store = null) {}

        /**
         * @param string $name
         * @return mixed
         */
        public function __get(string $name): mixed {}

        /**
         * @param string $name
         * @return bool
         */
        public function __isset(string $name): bool {}

        /**
         * @param string $name
         * @param mixed $value
         * @return void
         */
        public function __set(string $name, mixed $value): void {}

        /**
         * @return array{value: string, mutable: bool}
         */
        public function type(): array {}

        /**
         * @return mixed
         */
        public function valueOf(): mixed {}
    }

    final class Instance {
        public readonly mixed $exports;

        /**
         * @param \Wasm\Module $module
         * @param array|null $imports
         * @param \Wasm\Store|null $store
         */
        public function __construct(\Wasm\Module $module, ?array $imports = null, ?\Wasm\Store $store = null) {}
    }

    /**
     * Linear memory, like JS `WebAssembly.Memory`.
     *
     * PHP has no shared ArrayBuffer, so reads and writes copy bytes in and out.
     */
    final class Memory {
        /**
         * @param array{initial?: int, minimum?: int, maximum?: int, address?: 'i32'|'i64'} $descriptor
         *
         * @param \Wasm\Store|null $store
         */
        public function __construct(array $descriptor, ?\Wasm\Store $store = null) {}

        /**
         * A copy of the whole memory.
         *
         * @return string
         */
        public function buffer(): string {}

        /**
         * @return int
         */
        public function byteLength(): int {}

        /**
         * Grows the memory by `delta` pages and returns the previous size in pages.
         *
         * @param int $delta
         * @return int
         */
        public function grow(int $delta): int {}

        /**
         * @param int $offset
         * @param int $length
         * @return string
         */
        public function read(int $offset, int $length): string {}

        /**
         * The memory's type, with its current size in pages as `minimum`.
         *
         * @return array{minimum: int, maximum?: int, address?: 'i64'}
         */
        public function type(): array {}

        /**
         * @param int $offset
         * @param string $data
         * @return void
         */
        public function write(int $offset, string $data): void {}
    }

    final class Module {
        /**
         * Compiles a wasm binary or WAT text.
         *
         * @param string $bytes
         */
        public function __construct(string $bytes) {}

        /**
         * @return list<string>
         *
         * @param string $name
         */
        public function customSections(string $name): array {}

        /**
         * Each entry's `type` is shaped like the JS type reflection proposal:
         * `{parameters, results}` for a function, `{value, mutable}` for a global,
         * `{minimum, maximum?}` for a memory, `{element, minimum, maximum?}` for a
         * table and `{parameters}` for a tag.
         *
         * @return list<array{name: string, kind: string, type: array<string, mixed>}>
         */
        public function exports(): array {}

        /**
         * Compiles a wasm binary or WAT file, like `new Module(file_get_contents($path))`.
         *
         * Reads local files only and honours open_basedir. Use file_get_contents()
         * for stream wrappers such as phar:// or compress.zlib://.
         *
         * @param string $path
         * @return \Wasm\Module
         */
        public static function fromFile(string $path): \Wasm\Module {}

        /**
         * `type` is shaped as in exports().
         *
         * @return list<array{module: string, name: string, kind: string, type: array<string, mixed>}>
         */
        public function imports(): array {}
    }

    /**
     * Turns compiled modules and components into precompiled artifacts and back,
     * so production can load them without compiling.
     *
     * An artifact is native machine code. Only load artifacts you built yourself
     * and stored where nobody else can write: a crafted artifact can run any code
     * with the rights of PHP, like loading an extension. The checksum in an artifact
     * catches corruption, not tampering.
     *
     * An artifact loads only on a host with the same OS and CPU architecture, a
     * CPU with at least the features of the one that built it, the same wasmtime
     * major version and ext-wasm engine settings that wasmtime accepts as
     * compatible; anything else is a CompileError.
     */
    final class Serializer {
        public function __construct() {}

        /**
         * @param string $artifact
         * @return \Wasm\Component\Component
         */
        public function deserializeComponent(string $artifact): \Wasm\Component\Component {}

        /**
         * Reads local files only and honours open_basedir.
         *
         * @param string $path
         * @return \Wasm\Component\Component
         */
        public function deserializeComponentFile(string $path): \Wasm\Component\Component {}

        /**
         * @param string $artifact
         * @return \Wasm\Module
         */
        public function deserializeModule(string $artifact): \Wasm\Module {}

        /**
         * Reads local files only and honours open_basedir.
         *
         * @param string $path
         * @return \Wasm\Module
         */
        public function deserializeModuleFile(string $path): \Wasm\Module {}

        /**
         * @param \Wasm\Component\Component $component
         * @return string
         */
        public function serializeComponent(\Wasm\Component\Component $component): string {}

        /**
         * @param \Wasm\Module $module
         * @return string
         */
        public function serializeModule(\Wasm\Module $module): string {}
    }

    /**
     * Groups wasm objects so they can be combined.
     *
     * An object created without a store joins the store of the wasm objects it
     * is built from. Otherwise an instance gets a store of its own, and a
     * Memory, Table, GlobalVar or Tag joins the store all such standalone
     * objects share. wasmtime frees memory one whole store at a time, when no
     * object in it is left.
     */
    final class Store {
        public function __construct() {}
    }

    /**
     * Marks a function import that may suspend the calling Fiber, like JS
     * `WebAssembly.Suspending`.
     *
     * Every other Fiber keeps running while wasm waits for the callback. Calling
     * into the instance's store again before the callback returns throws a
     * RuntimeError, so run one instance per Fiber.
     */
    final class Suspending {
        /**
         * @param mixed $callback
         */
        public function __construct(mixed $callback) {}
    }

    /**
     * A table of references, like JS `WebAssembly.Table`.
     */
    final class Table {
        /**
         * `element` is `funcref`, `externref`, `nullfuncref`, `nullexternref`,
         * `(ref func)` or `(ref extern)`; the last two need a `$value`.
         *
         * @param array{element: string, initial?: int, minimum?: int, maximum?: int, address?: 'i32'|'i64'} $descriptor
         *
         * @param mixed $value
         * @param \Wasm\Store|null $store
         */
        public function __construct(array $descriptor, mixed $value = null, ?\Wasm\Store $store = null) {}

        /**
         * @param int $index
         * @return mixed
         */
        public function get(int $index): mixed {}

        /**
         * Grows the table by `delta` entries and returns the previous length.
         *
         * @param int $delta
         * @param mixed $value
         * @return int
         */
        public function grow(int $delta, mixed $value = null): int {}

        /**
         * @return int
         */
        public function length(): int {}

        /**
         * @param int $index
         * @param mixed $value
         * @return void
         */
        public function set(int $index, mixed $value = null): void {}

        /**
         * The table's type, with its current length as `minimum`.
         *
         * @return array{element: string, minimum: int, maximum?: int, address?: 'i64'}
         */
        public function type(): array {}
    }

    /**
     * An exception tag, like JS `WebAssembly.Tag`.
     */
    final class Tag {
        /**
         * @param array{parameters: list<string>} $descriptor
         *
         * @param \Wasm\Store|null $store
         */
        public function __construct(array $descriptor, ?\Wasm\Store $store = null) {}

        /**
         * @return array{parameters: list<string>}
         */
        public function type(): array {}
    }

    /**
     * A WASI environment for one run of one module or component, like Node's `WASI`.
     *
     * Nothing of the host is visible to the program except what is passed here:
     * no environment, no stdio and no files outside the preopened directories.
     * stdout and stderr are captured and read after the run. A core module gets
     * WASI preview1 through `getImportObject()`, a component gets preview2 when
     * the Wasi object is passed to `Wasm\Component\Instance`.
     */
    final class Wasi {
        /**
         * @param list<string>|null $args argv, including the program name
         * @param array<string, string>|null $env
         * @param array<string, string|array{path: string, writable?: bool}>|null $preopens guest path => host path
         * @param int|null $outputLimit bytes kept of stdout and of stderr, 16 MiB by default
         * @param list<string>|null $httpHosts hosts a component may send HTTP requests to: "host", "host:port" or "*.domain"; checked by name, not by the address it resolves to
         * @param list<string>|null $tcpHosts destinations a component may open TCP connections to: "host:port", "ip:port" or "network/prefix:port", with * for any port; a host is checked by the addresses it resolves to when the component connects
         * @param list<string>|null $udpHosts destinations a component may send UDP datagrams to and receive them from, in the same form; a host is resolved once, when this object is created
         *
         * @param string|null $stdin
         */
        public function __construct(?array $args = null, ?array $env = null, ?array $preopens = null, ?string $stdin = null, ?int $outputLimit = null, ?array $httpHosts = null, ?array $tcpHosts = null, ?array $udpHosts = null) {}

        /**
         * The preview1 functions for a core module.
         *
         * @return array{wasi_snapshot_preview1: array<string, \Wasm\Func>}
         */
        public function getImportObject(): array {}

        /**
         * Runs `_initialize` when the module exports it, for modules used as a library.
         *
         * @param \Wasm\Instance $instance
         * @return void
         */
        public function initialize(\Wasm\Instance $instance): void {}

        /**
         * Runs `_start` of a module, or `wasi:cli/run` of a component, and
         * returns the exit code.
         *
         * @param \Wasm\Instance|\Wasm\Component\Instance $instance
         *
         * @return int
         */
        public function start(mixed $instance): int {}

        /**
         * @return string
         */
        public function stderr(): string {}

        /**
         * @return string
         */
        public function stdout(): string {}
    }

    /**
     * @param string $bytes
     * @return \Wasm\Module
     */
    function compile(string $bytes): \Wasm\Module {}

    /**
     * Like JS `WebAssembly.instantiate()`: bytes give `['module' => Module, 'instance' => Instance]`,
     * a Module gives the Instance.
     *
     * @return Instance|array{module: Module, instance: Instance}
     *
     * @param mixed $source
     * @param array|null $imports
     */
    function instantiate(mixed $source, ?array $imports = null): mixed {}

    /**
     * Whether `bytes` is a valid wasm binary or WAT module.
     *
     * @param string $bytes
     * @return bool
     */
    function validate(string $bytes): bool {}
}

namespace Wasm\Component {
    /**
     * A compiled WebAssembly component.
     *
     * Compile once and instantiate as often as needed, like `Wasm\Module`.
     */
    final class Component {
        /**
         * Compiles a component binary or WAT text.
         *
         * @param string $bytes
         */
        public function __construct(string $bytes) {}

        /**
         * @return list<array{name: string, kind: string, type?: string, functions?: list<array{name: string, kind: string, type?: string}>}>
         */
        public function exports(): array {}

        /**
         * Compiles a component file, like `new Component(file_get_contents($path))`.
         *
         * Reads local files only and honours open_basedir.
         *
         * @param string $path
         * @return \Wasm\Component\Component
         */
        public static function fromFile(string $path): \Wasm\Component\Component {}

        /**
         * @return list<array{name: string, kind: string, type?: string, functions?: list<array{name: string, kind: string, type?: string}>}>
         */
        public function imports(): array {}
    }

    /**
     * A WIT `error-context` a component handed over. It has nothing to read,
     * because wasmtime 49 gives the host no access to the debug message, and a
     * component cannot be given one back. Each one received is a new object, so
     * neither `==` nor `===` tells whether two are the same error-context.
     */
    final class ErrorContext {
        public function __construct() {}
    }

    /**
     * The exports of a component instance, or of one interface it exports.
     *
     * Functions are camelCase methods; `get()` takes any export by its WIT name,
     * with or without version.
     */
    final class Exports implements \IteratorAggregate {
        /**
         * @param string $name
         * @param array $arguments
         * @return mixed
         */
        public function __call(string $name, array $arguments): mixed {}

        public function __construct() {}

        /**
         * @return \Wasm\Component\Func|\Wasm\Component\Exports
         *
         * @param string $name
         */
        public function get(string $name): mixed {}

        /**
         * Every export by WIT name. An aggregate rather than an Iterator, so WIT
         * functions called next or current stay callable as methods.
         *
         * @return \Wasm\Component\ExportsIterator
         */
        public function getIterator(): \Wasm\Component\ExportsIterator {}
    }

    /**
     * Iterates the exports of a component instance by WIT name.
     */
    final class ExportsIterator implements \Iterator {
        public function __construct() {}

        /**
         * @return \Wasm\Component\Func|\Wasm\Component\Exports|null
         */
        public function current(): mixed {}

        /**
         * @return string|null
         */
        public function key(): ?string {}

        /**
         * @return void
         */
        public function next(): void {}

        /**
         * @return void
         */
        public function rewind(): void {}

        /**
         * @return bool
         */
        public function valid(): bool {}
    }

    /**
     * An exported component function, callable from PHP.
     */
    final class Func {
        public function __construct() {}

        /**
         * @param mixed $args
         * @return mixed
         */
        public function __invoke(mixed ...$args): mixed {}

        /**
         * The function's WIT type.
         *
         * @return \Wasm\Component\Type\FunctionType
         */
        public function type(): mixed {}
    }

    /**
     * A `future<T>` a component returned. `await()` runs the component until
     * its value is there and returns it, the same value on every call.
     */
    final class Future {
        public function __construct() {}

        /**
         * The value of the future, once the component wrote it.
         *
         * @return mixed
         */
        public function await(): mixed {}
    }

    /**
     * An instance of a component, with a store of its own.
     */
    final class Instance {
        public readonly mixed $exports;

        /**
         * @param array<string, callable|array<string, callable>>|null $imports
         * @param \Wasm\Wasi|null $wasi provides every `wasi:*` import, as WASI preview2
         *
         * @param \Wasm\Component\Component $component
         */
        public function __construct(\Wasm\Component\Component $component, ?array $imports = null, ?\Wasm\Wasi $wasi = null) {}

        /**
         * Hands `request` to the component's `wasi:http/incoming-handler` and
         * returns its response.
         *
         * @param \Wasm\Component\Http\Request $request
         * @return \Wasm\Component\Http\Response
         */
        public function handle(\Wasm\Component\Http\Request $request): \Wasm\Component\Http\Response {}
    }

    /**
     * A handle to a resource owned by a component instance. Methods call the
     * component; `drop()` releases the handle, as does the destructor.
     */
    final class Resource {
        /**
         * @param string $name
         * @param array $arguments
         * @return mixed
         */
        public function __call(string $name, array $arguments): mixed {}

        /**
         * @return void
         */
        public function __clone(): void {}

        public function __construct() {}

        /**
         * Calls the method `name` by its WIT name, for a method called `drop`.
         *
         * @param string $name
         * @param mixed $args
         * @return mixed
         */
        public function call(string $name, mixed ...$args): mixed {}

        /**
         * Releases the handle; the component runs its destructor for the resource.
         *
         * @return void
         */
        public function drop(): void {}
    }

    /**
     * A resource type a component exports: `new(...)` constructs it, and its
     * static functions are camelCase methods.
     */
    final class ResourceClass {
        /**
         * @param string $name
         * @param array $arguments
         * @return mixed
         */
        public function __call(string $name, array $arguments): mixed {}

        /**
         * @return void
         */
        public function __clone(): void {}

        public function __construct() {}

        /**
         * Calls the resource's constructor.
         *
         * @param mixed $args
         * @return mixed
         */
        public function new(mixed ...$args): mixed {}
    }

    /**
     * A value of a WIT `result` inside another value: ok with a value, or err
     * with a payload.
     */
    final class Result {
        /**
         * Whether this is an ok result, as a property for var_dump() and assertEquals().
         *
         * @var bool
         */
        public readonly bool $ok;

        /**
         * The ok value or the err payload, as a property for var_dump() and assertEquals().
         *
         * @var mixed
         */
        public readonly mixed $payload;

        public function __construct() {}

        /**
         * @param mixed $error
         * @return \Wasm\Component\Result
         */
        public static function err(mixed $error = null): \Wasm\Component\Result {}

        /**
         * The err payload; throws for an ok result.
         *
         * @return mixed
         */
        public function error(): mixed {}

        /**
         * @return bool
         */
        public function isErr(): bool {}

        /**
         * @return bool
         */
        public function isOk(): bool {}

        /**
         * @param mixed $value
         * @return \Wasm\Component\Result
         */
        public static function ok(mixed $value = null): \Wasm\Component\Result {}

        /**
         * The ok value; throws the err payload as a ComponentError.
         *
         * @return mixed
         */
        public function value(): mixed {}
    }

    /**
     * A `stream<T>` a component returned. `read()` gives the next chunk:
     * a binary string for `stream<u8>`, a list of values otherwise, `null` at
     * the end. Iterating gives the chunks too.
     */
    final class Stream implements \Iterator {
        public function __construct() {}

        /**
         * @return string|list<mixed>|null
         */
        public function current(): mixed {}

        /**
         * @return int
         */
        public function key(): int {}

        /**
         * @return void
         */
        public function next(): void {}

        /**
         * The next chunk, or `null` once the stream ended.
         *
         * @return string|list<mixed>|null
         */
        public function read(): mixed {}

        /**
         * Starts reading; a stream cannot be read twice, so later calls do nothing.
         *
         * @return void
         */
        public function rewind(): void {}

        /**
         * @return bool
         */
        public function valid(): bool {}
    }

    /**
     * A value of a WIT `variant`: the name of its case and the case's payload.
     */
    final class Variant {
        public readonly string $tag;

        public readonly mixed $value;

        /**
         * @param string $tag
         * @param mixed $value
         */
        public function __construct(string $tag, mixed $value = null) {}
    }
}

namespace Wasm\Component\Http {
    /**
     * An HTTP request for a component, like `new Request('GET', 'https://example.com/')`.
     *
     * Header names are lowercase and every name maps to a list of values.
     */
    final class Request {
        public readonly string $body;

        /**
         * @return array<string, list<string>>
         *
         * @var mixed
         */
        public readonly mixed $headers;

        public readonly string $method;

        public readonly string $url;

        /**
         * @param array<string, string|list<string>>|null $headers
         *
         * @param string $method
         * @param string $url
         * @param string|null $body
         */
        public function __construct(string $method, string $url, ?array $headers = null, ?string $body = null) {}
    }

    /**
     * The HTTP response of a component.
     */
    final class Response {
        public readonly string $body;

        /**
         * @return array<string, list<string>>
         *
         * @var mixed
         */
        public readonly mixed $headers;

        public readonly int $status;

        /**
         * @param array<string, string|list<string>>|null $headers
         *
         * @param int $status
         * @param string|null $body
         */
        public function __construct(int $status, ?array $headers = null, ?string $body = null) {}
    }
}

namespace Wasm\Component\Type {
    /**
     * A WIT function type: its parameters by name and its result.
     */
    final class FunctionType {
        /**
         * @return array<string, \Wasm\Component\Type\ValueType>
         *
         * @var mixed
         */
        public readonly mixed $params;

        /**
         * @return \Wasm\Component\Type\ValueType|null
         *
         * @var mixed
         */
        public readonly mixed $result;

        public function __construct() {}

        /**
         * The type as WIT, like `func(markdown: string) -> string`.
         *
         * @return string
         */
        public function __toString(): string {}
    }

    /**
     * A WIT value type. `kind` is the WIT keyword; the other properties are set
     * for the kinds they belong to and null otherwise. Fixed-length lists only
     * report their kind: components using them do not compile yet.
     */
    final class ValueType {
        /**
         * @return array<string, \Wasm\Component\Type\ValueType|null>|null
         *
         * @var mixed
         */
        public readonly mixed $cases;

        /**
         * The element of a list or stream, the value of an option or future, or
         * the value type of a map.
         *
         * @return \Wasm\Component\Type\ValueType|null
         *
         * @var mixed
         */
        public readonly mixed $element;

        /**
         * @return \Wasm\Component\Type\ValueType|null
         *
         * @var mixed
         */
        public readonly mixed $err;

        /**
         * @return array<string, \Wasm\Component\Type\ValueType>|null
         *
         * @var mixed
         */
        public readonly mixed $fields;

        /**
         * The key type of a map.
         *
         * @return \Wasm\Component\Type\ValueType|null
         *
         * @var mixed
         */
        public readonly mixed $key;

        public readonly string $kind;

        /**
         * The name the component gives the type, if any.
         *
         * @var string|null
         */
        public readonly ?string $name;

        /**
         * @return list<string>|null
         *
         * @var mixed
         */
        public readonly mixed $names;

        /**
         * @return \Wasm\Component\Type\ValueType|null
         *
         * @var mixed
         */
        public readonly mixed $ok;

        /**
         * The resource of an own or borrow handle.
         *
         * @var string|null
         */
        public readonly ?string $resource;

        /**
         * @return list<\Wasm\Component\Type\ValueType>|null
         *
         * @var mixed
         */
        public readonly mixed $types;

        public function __construct() {}
    }
}

namespace Wasm\Exception {
    class CompileError extends \Wasm\Exception\WasmException {
        /**
         * @param string|null $message
         * @param int|null $code
         * @param mixed $previous
         */
        public function __construct(?string $message = null, ?int $code = null, mixed $previous = null) {}
    }

    /**
     * The err of a component function whose own return type is a `result`.
     *
     * A PHP import throws it to return an err to the component.
     *
     * @property mixed $payload
     */
    class ComponentError extends \Wasm\Exception\WasmException {
        public $payload = null;

        /**
         * @param mixed $payload
         */
        public function __construct(mixed $payload = null) {}
    }

    class LinkError extends \Wasm\Exception\WasmException {
        /**
         * @param string|null $message
         * @param int|null $code
         * @param mixed $previous
         */
        public function __construct(?string $message = null, ?int $code = null, mixed $previous = null) {}
    }

    class RuntimeError extends \Wasm\Exception\WasmException {
        /**
         * @param string|null $message
         * @param int|null $code
         * @param mixed $previous
         */
        public function __construct(?string $message = null, ?int $code = null, mixed $previous = null) {}
    }

    class WasmException extends \Exception {
        /**
         * @param string|null $message
         * @param int|null $code
         * @param mixed $previous
         */
        public function __construct(?string $message = null, ?int $code = null, mixed $previous = null) {}
    }

    /**
     * A wasm exception, like JS `WebAssembly.Exception`.
     *
     * Thrown in PHP when a wasm exception escapes to PHP, and thrown by a PHP
     * callback to raise an exception that wasm code can catch.
     *
     * @property \Wasm\Tag $tag
     * @property list<mixed> $payload
     */
    class WasmThrow extends \Wasm\Exception\WasmException {
        public $payload = null;

        public $tag = null;

        /**
         * @param \Wasm\Tag $tag
         * @param array|null $payload
         */
        public function __construct(\Wasm\Tag $tag, ?array $payload = null) {}
    }
}
