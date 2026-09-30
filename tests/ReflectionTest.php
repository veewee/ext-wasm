<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Module;

final class ReflectionTest extends TestCase
{
    public function test_it_lists_exports_in_module_order(): void
    {
        $module = new Module(<<<'EOWAT'
            (module
              (func (export "run"))
              (memory (export "memory") 1)
              (global (export "g") i32 (i32.const 0))
              (table (export "t") 1 funcref)
              (tag (export "e")))
            EOWAT);

        self::assertSame([
            ['name' => 'run', 'kind' => 'function'],
            ['name' => 'memory', 'kind' => 'memory'],
            ['name' => 'g', 'kind' => 'global'],
            ['name' => 't', 'kind' => 'table'],
            ['name' => 'e', 'kind' => 'tag'],
        ], Module::exports($module));
    }

    public function test_it_lists_imports(): void
    {
        $module = new Module(<<<'EOWAT'
            (module
              (import "env" "log" (func (param i32)))
              (import "js" "mem" (memory 1)))
            EOWAT);

        self::assertSame([
            ['module' => 'env', 'name' => 'log', 'kind' => 'function'],
            ['module' => 'js', 'name' => 'mem', 'kind' => 'memory'],
        ], Module::imports($module));
    }

    public function test_it_reads_custom_sections(): void
    {
        $module = new Module(self::withCustomSections([['meta', 'first'], ['other', 'x'], ['meta', "sec\0ond"]]));

        self::assertSame(['first', "sec\0ond"], Module::customSections($module, 'meta'));
        self::assertSame([], Module::customSections($module, 'missing'));
    }

    /** @param list<array{string, string}> $sections */
    private static function withCustomSections(array $sections): string
    {
        $binary = "\0asm\x01\0\0\0";
        foreach ($sections as [$name, $payload]) {
            $content = chr(strlen($name)) . $name . $payload;
            $binary .= "\0" . chr(strlen($content)) . $content;
        }

        return $binary;
    }
}
