<?php

declare(strict_types=1);

use Wasm\Component\Component;
use Wasm\Component\Instance;
use Wasm\Component\Resource;

/**
 * Reads a git repository through libgit2, compiled to a component from
 * src/git.c. The component sees the repository directory, read-only, and
 * nothing else of the host.
 *
 * The repository stays open between calls, so the second call no longer
 * pays for reading the config, the refs and the pack indexes.
 *
 * @phpstan-type Signature array{name: string, email: string, time: int, offsetMinutes: int}
 * @phpstan-type Commit array{hash: string, tree: string, parents: list<string>, author: Signature, committer: Signature, message: string}
 * @phpstan-type TreeEntry array{name: string, hash: string, mode: int, kind: string}
 *
 * Every method throws a Wasm\Exception\ComponentError with libgit2's message
 * as its payload when the revision or the object does not exist.
 */
final class Git
{
    private const GUEST_DIR = '/repo';

    private readonly Instance $instance;
    private readonly Resource $repository;

    /** @param string $path a working tree or a bare repository */
    public function __construct(string $path, string $wasmFile = __DIR__ . '/git.wasm')
    {
        $wasi = new Wasm\Wasi(preopens: [self::GUEST_DIR => $path]);
        $this->instance = new Instance(Component::fromFile($wasmFile), wasi: $wasi);
        $this->repository = $this->instance->exports->get('docs:git/repositories')->get('repository')->open(self::GUEST_DIR);
    }

    /** The full hash of a revision, such as HEAD, main~3 or HEAD:README.md. */
    public function resolve(string $revision): string
    {
        return $this->repository->resolve($revision);
    }

    /** @return Commit */
    public function commit(string $revision): array
    {
        return $this->repository->commit($revision);
    }

    /** @return list<Commit> newest first by commit time */
    public function log(string $revision, int $limit): array
    {
        return $this->repository->log($revision, $limit);
    }

    /** @return list<TreeEntry> */
    public function tree(string $revision): array
    {
        return $this->repository->tree($revision);
    }

    /** The bytes of a file, such as blob('HEAD:README.md'). */
    public function blob(string $revision): string
    {
        return $this->repository->blob($revision);
    }
}
