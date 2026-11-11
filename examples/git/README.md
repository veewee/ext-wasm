# Reading git repositories with libgit2

This example compiles [libgit2](https://libgit2.org), a C library, into a component and reads git repositories with it from PHP: commits, logs, trees and file contents. There is no git binary, no FFI and no native libgit2 on the machine. The other component examples are written in Rust; this one shows an existing C library behind a WIT interface.

The interface in `wit/git.wit` has a `repository` resource. Opening it reads the repository's config and refs once. After that the repository stays open inside the component, together with the pack indexes it loaded and libgit2's object cache, so later calls only do the work they ask for. `src/git.c` is the binding layer, about 200 lines that turn libgit2's objects into WIT records.

The component sees one directory, the repository, read-only.

## Running it

Install the extension as described in the [main README](../../README.md). The compiled `git.wasm` is checked in, so running it needs no C toolchain.

```sh
php examples/git/log.php /path/to/repo
php examples/git/log.php /path/to/repo main~10 5
```

## From PHP

`Git.php` wraps the component:

```php
$git = new Git('/path/to/repo');
$git->resolve('HEAD');                 // 'e8281019778c7579d5254bc4f6db80c3bfae538c'
$git->commit('HEAD');                  // ['hash' => ..., 'tree' => ..., 'parents' => [...], 'author' => [...], 'committer' => [...], 'message' => ...]
$git->log('HEAD', 100);                // the same records, newest first
$git->tree('HEAD:src');                // [['name' => 'main.c', 'hash' => ..., 'mode' => 0o100644, 'kind' => 'blob'], ...]
$git->blob('HEAD:README.md');          // the bytes of the file
```

Every method takes a revision as git understands it, such as `HEAD`, `main~3`, `v1.0^{commit}` or `HEAD:path/to/file`. A revision that does not exist throws a `Wasm\Exception\ComponentError` with libgit2's message as its `payload`.

Keep one `Git` object for as many reads as you can, since every new one creates an instance and opens the repository again. Compiling `git.wasm` takes about 100 ms, so turn on `wasm.cache` or use `Wasm\Serializer` to keep a request from paying for it.

## Limits

- Reads only, and only local repositories: the component is built without the network transports.
- WASI has no mmap, so libgit2 reads a pack index into memory when it first needs it. On a large repository that is tens of megabytes, held for as long as the repository is open.
- `blob()` returns a `list<u8>`, which crosses into PHP one byte at a time and stops at about 4 MB.

## Changing the C code

Edit `src/git.c` or `wit/git.wit` and rebuild:

```sh
examples/git/build.sh
```

It needs curl, cmake and make. On first use it downloads [wasi-sdk](https://github.com/WebAssembly/wasi-sdk) and [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen) into `build/`, together with libgit2 1.9.4 and the patches [wasm-git](https://github.com/petersalomonsen/wasm-git) wrote to run libgit2 on WASI. Building libgit2 takes a minute or two. wit-bindgen generates the C side of the WIT interface into `build/bindings`.
