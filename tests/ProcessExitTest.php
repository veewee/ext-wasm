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
        $script = <<<'PHP'
            <?php
            $functions = '';
            for ($i = 0; $i < 500; $i++) {
                $functions .= "(func (export \"f$i\") (result i32) (i32.const $i))\n";
            }
            echo (new Wasm\Instance(new Wasm\Module("(module $functions)")))->exports->f42();
            PHP;

        for ($run = 1; $run <= 15; $run++) {
            $output = $this->runPhp($script, $exitCode, ['wasm.cache' => '0']);
            self::assertSame(0, $exitCode, "run $run exited with $exitCode: $output");
            self::assertSame('42', $output);
        }
    }
}
