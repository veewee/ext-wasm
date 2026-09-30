# Python in PHP

This example runs Python code inside PHP. The interpreter is CPython 3.12 compiled to WASI by [VMware Labs](https://github.com/vmware-labs/webassembly-language-runtimes), with the standard library embedded in the 26 MB module, so it needs no files on disk. It runs sandboxed: Python sees no environment variables and no files of the host.

## Running it

Install the extension as described in the [main README](../../README.md), then download the module and run some Python:

```sh
examples/python/download.sh
php examples/python/run.php 'print(6 * 7)'
echo '{"a": [1, 2, 3]}' | php examples/python/run.php 'import json, sys; print(sum(json.load(sys.stdin)["a"]))'
```

stdin is passed on to Python, and the script exits with Python's exit code. Compiling the module takes about a second the first time; after that the [compilation cache](../../README.md#compilation-cache) loads it in a fraction of that.
