<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;

require_once __DIR__ . '/RunsPhpInSubprocess.php';

/** The compilation cache is configured per process, so every case runs in a fresh one. */
final class CompilationCacheTest extends TestCase
{
    use RunsPhpInSubprocess;

    private string $directory;

    protected function setUp(): void
    {
        $this->directory = sys_get_temp_dir() . '/wasm-cache-test-' . bin2hex(random_bytes(6));
    }

    protected function tearDown(): void
    {
        if (!is_dir($this->directory)) {
            return;
        }
        $files = new \RecursiveIteratorIterator(
            new \RecursiveDirectoryIterator($this->directory, \FilesystemIterator::SKIP_DOTS),
            \RecursiveIteratorIterator::CHILD_FIRST,
        );
        foreach ($files as $file) {
            $file->isDir() ? rmdir($file->getPathname()) : unlink($file->getPathname());
        }
        rmdir($this->directory);
    }

    public function test_compiled_modules_are_stored_in_the_cache_directory(): void
    {
        $this->compileLargeModule(['wasm.cache_dir' => $this->directory]);

        self::assertNotEmpty($this->cachedFiles());
    }

    public function test_a_second_process_loads_the_module_from_the_cache(): void
    {
        $settings = ['wasm.cache_dir' => $this->directory];
        $cold = $this->compileLargeModule($settings);
        $warm = $this->compileLargeModule($settings);

        self::assertLessThan($cold / 3, $warm, "cold compile took {$cold}s, cached load {$warm}s");
    }

    public function test_the_cache_can_be_disabled(): void
    {
        $this->compileLargeModule(['wasm.cache' => '0', 'wasm.cache_dir' => $this->directory]);

        self::assertSame([], $this->cachedFiles());
    }

    public function test_an_unusable_cache_directory_does_not_stop_compilation(): void
    {
        $file = tempnam(sys_get_temp_dir(), 'wasm-not-a-directory');
        try {
            $seconds = $this->compileLargeModule(['wasm.cache_dir' => $file . '/cache']);
            self::assertGreaterThan(0, $seconds);
        } finally {
            unlink($file);
        }
    }

    public function test_the_settings_are_reported_by_ini_get(): void
    {
        $output = $this->runPhp('<?php var_export([ini_get("wasm.cache"), ini_get("wasm.cache_dir")]);');

        self::assertSame("array (\n  0 => '1',\n  1 => '',\n)", $output);
    }

    /**
     * @param array<string, string> $settings
     * @return float seconds spent in `new Module`
     */
    private function compileLargeModule(array $settings): float
    {
        $script = <<<'PHP'
            <?php
            $functions = '';
            for ($i = 0; $i < 3000; $i++) {
                $functions .= "(func (export \"f$i\") (param i32) (result i32) (i32.mul (i32.add (local.get 0) (i32.const $i)) (i32.const 3)))\n";
            }
            $started = microtime(true);
            $module = new Wasm\Module("(module $functions)");
            $seconds = microtime(true) - $started;
            echo (new Wasm\Instance($module))->exports->f7(1) === 24 ? $seconds : 'wrong result';
            PHP;

        $output = $this->runPhp($script, $exitCode, $settings);
        self::assertSame(0, $exitCode, $output);
        self::assertIsNumeric($output, $output);

        return (float) $output;
    }

    /** @return list<string> */
    private function cachedFiles(): array
    {
        if (!is_dir($this->directory)) {
            return [];
        }
        $files = new \RecursiveIteratorIterator(new \RecursiveDirectoryIterator($this->directory, \FilesystemIterator::SKIP_DOTS));

        return array_values(array_map(fn (\SplFileInfo $file) => $file->getPathname(), iterator_to_array($files)));
    }
}
