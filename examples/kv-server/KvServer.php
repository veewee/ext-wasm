<?php

declare(strict_types=1);

use Wasm\Component\Component;
use Wasm\Component\Exports;
use Wasm\Component\Instance;

/**
 * A Redis compatible key-value server by the async Rust component in src/lib.rs.
 *
 * wit/kv.wit declares serve(input: stream<u8>) -> stream<u8>: PHP passes what
 * the client sends and writes back what the component replies. The sockets
 * stay in PHP; the component only speaks the protocol. One instance serves
 * every connection, so the data outlives a connection.
 */
final class KvServer
{
    private readonly Exports $connection;

    public function __construct(string $wasmFile = __DIR__ . '/kv-server.wasm')
    {
        $instance = new Instance(Component::fromFile($wasmFile), wasi: new Wasm\Wasi());
        $this->connection = $instance->exports->get('example:kv-server/connection');
    }

    /**
     * Serves one client until it sends QUIT or disconnects.
     *
     * @param resource $client a connected socket stream
     */
    public function serve($client): void
    {
        foreach ($this->connection->serve(self::read($client)) as $reply) {
            if (@fwrite($client, $reply) === false) {
                return;
            }
        }
    }

    /** @return \Generator<string> what the client sends, as it arrives */
    private static function read($client): \Generator
    {
        while (($chunk = fread($client, 16 * 1024)) !== false && $chunk !== '') {
            yield $chunk;
        }
    }
}
