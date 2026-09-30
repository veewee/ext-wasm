<?php

declare(strict_types=1);

namespace Example\Mago;

use Wasm\Exports;
use Wasm\Instance;
use Wasm\Memory;
use Wasm\Module;

/**
 * A small PHP host for a wasm-bindgen module that was built for JavaScript.
 *
 * wasm-bindgen keeps JS values in a heap on the host side and only hands wasm
 * an index into it. This class reimplements that heap and the imports mago
 * needs to return strings, arrays and objects. Any other import throws with its
 * name, so a mago build that needs more shows up right away.
 */
final class WasmBindgenHost
{
    /** wasm-bindgen reserves the first slots, followed by undefined, null, true and false. */
    private const RESERVED = 1028;

    private readonly Exports $exports;
    private readonly \stdClass $undefined;
    /** @var array<int, mixed> */
    private array $heap;
    private int $next;

    /**
     * @param string $glue The generated `*_bg.js` file. Import names carry a
     *                     hash that changes per build, and a few imports share
     *                     a name and signature, so their JS body tells them apart.
     */
    public function __construct(Module $module, string $glue)
    {
        $this->undefined = new \stdClass();
        $this->heap = array_fill(0, self::RESERVED - 4, $this->undefined);
        array_push($this->heap, $this->undefined, null, true, false);
        $this->next = count($this->heap);

        $bodies = self::functionBodies($glue);
        $imports = [];
        foreach (Module::imports($module) as $import) {
            $imports[$import['module']][$import['name']] = $this->import($import['name'], $bodies[$import['name']] ?? '');
        }
        $this->exports = (new Instance($module, $imports))->exports;
    }

    /** Calls an export that takes strings and returns a string, like mago's `format`. */
    public function callReturningString(string $name, string ...$args): string
    {
        $retptr = $this->exports->__wbindgen_add_to_stack_pointer(-16);
        try {
            $this->exports->{$name}($retptr, ...$this->passStrings($args));
            [$ptr, $len, $error, $failed] = array_values(unpack('V4', $this->memory()->read($retptr, 16)));
            if ($failed) {
                throw $this->toThrowable($this->take($error));
            }
            $result = $this->memory()->read($ptr, $len);
            $this->exports->__wbindgen_export4($ptr, $len, 1);

            return $result;
        } finally {
            $this->exports->__wbindgen_add_to_stack_pointer(16);
        }
    }

    /** Calls an export that takes strings and returns a JS value, like mago's `analyze`. */
    public function callReturningValue(string $name, string ...$args): mixed
    {
        $retptr = $this->exports->__wbindgen_add_to_stack_pointer(-16);
        try {
            $this->exports->{$name}($retptr, ...$this->passStrings($args));
            [$value, $error, $failed] = array_values(unpack('V3', $this->memory()->read($retptr, 12)));
            if ($failed) {
                throw $this->toThrowable($this->take($error));
            }

            return $this->toPhp($this->take($value));
        } finally {
            $this->exports->__wbindgen_add_to_stack_pointer(16);
        }
    }

    private function import(string $name, string $body): \Closure
    {
        $base = preg_replace('/_[0-9a-f]{16}$/', '', $name);

        return match (true) {
            str_contains($body, 'new Object()'), str_contains($body, 'new Array()') => fn (): int => $this->add(new \ArrayObject()),
            str_contains($body, '[arg1 >>> 0] = takeObject(arg2)') => function (int $target, int $index, int $value): void {
                $this->heap[$target][$index & 0xFFFFFFFF] = $this->take($value);
            },
            str_contains($body, '[takeObject(arg1)] = takeObject(arg2)') => function (int $target, int $key, int $value): void {
                $this->heap[$target][$this->take($key)] = $this->take($value);
            },
            $base === '__wbg_Error' => fn (int $ptr, int $len): int => $this->add(new \RuntimeException($this->string($ptr, $len))),
            $base === '__wbg___wbindgen_throw' => fn (int $ptr, int $len) => throw new \RuntimeException($this->string($ptr, $len)),
            // Casts from a wasm number or string to a JS value.
            str_contains($body, '`F64 -> Externref`') => fn (float $number): int => $this->add($number),
            str_contains($body, '`Ref(String) -> Externref`') => fn (int $ptr, int $len): int => $this->add($this->string($ptr, $len)),
            $base === '__wbindgen_object_clone_ref' => fn (int $index): int => $this->add($this->heap[$index]),
            $base === '__wbindgen_object_drop_ref' => function (int $index): void {
                $this->take($index);
            },
            default => fn () => throw new \LogicException("wasm-bindgen import $name is not implemented by this host"),
        };
    }

    /** @param list<string> $strings */
    private function passStrings(array $strings): array
    {
        $args = [];
        foreach ($strings as $string) {
            $ptr = $this->exports->__wbindgen_export(strlen($string), 1) & 0xFFFFFFFF;
            $this->memory()->write($ptr, $string);
            array_push($args, $ptr, strlen($string));
        }

        return $args;
    }

    private function add(mixed $value): int
    {
        if ($this->next === count($this->heap)) {
            $this->heap[] = count($this->heap) + 1;
        }
        $index = $this->next;
        $this->next = $this->heap[$index];
        $this->heap[$index] = $value;

        return $index;
    }

    private function take(int $index): mixed
    {
        $value = $this->heap[$index];
        if ($index >= self::RESERVED) {
            $this->heap[$index] = $this->next;
            $this->next = $index;
        }

        return $value;
    }

    private function string(int $ptr, int $len): string
    {
        return $this->memory()->read($ptr & 0xFFFFFFFF, $len);
    }

    private function memory(): Memory
    {
        return $this->exports->memory;
    }

    private function toPhp(mixed $value): mixed
    {
        if ($value instanceof \ArrayObject) {
            return array_map($this->toPhp(...), $value->getArrayCopy());
        }

        return $value === $this->undefined ? null : $value;
    }

    private function toThrowable(mixed $error): \Throwable
    {
        return $error instanceof \Throwable ? $error : new \RuntimeException((string) $this->toPhp($error));
    }

    /** @return array<string, string> */
    private static function functionBodies(string $glue): array
    {
        preg_match_all('/^export function (\w+)\([^)]*\) \{(.*?)^\}/ms', $glue, $matches, PREG_SET_ORDER);

        return array_column(array_map(fn (array $match) => [$match[1], $match[2]], $matches), 1, 0);
    }
}
