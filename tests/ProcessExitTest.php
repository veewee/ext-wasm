<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;

require_once __DIR__ . '/RunsPhpInSubprocess.php';

/**
 * PHP unloads extensions at shutdown while wasmtime's compile and cache
 * threads may still be running code from the extension. Exiting has to stay
 * clean anyway, every time.
 */
final class ProcessExitTest extends TestCase
{
    use RunsPhpInSubprocess;

    public function test_processes_exit_cleanly_after_compiling(): void
    {
        $this->assertCleanExits(['wasm.cache' => '0']);
    }

    public function test_processes_exit_cleanly_after_using_the_compilation_cache(): void
    {
        $directory = sys_get_temp_dir() . '/wasm-exit-test-' . bin2hex(random_bytes(6));
        $this->assertCleanExits(['wasm.cache_dir' => $directory]);
    }

    /** @param array<string, string> $settings */
    private function assertCleanExits(array $settings): void
    {
        $script = <<<'PHP'
            <?php
            $functions = '';
            for ($i = 0; $i < 500; $i++) {
                $functions .= "(func (export \"f$i\") (result i32) (i32.const $i))\n";
            }
            echo (new Wasm\Instance(new Wasm\Module("(module $functions)")))->exports->f42();
            PHP;

        for ($run = 1; $run <= 15; $run++) {
            $output = $this->runPhp($script, $exitCode, $settings);
            self::assertSame(0, $exitCode, "run $run exited with $exitCode: $output");
            self::assertSame('42', $output);
        }
    }
}
