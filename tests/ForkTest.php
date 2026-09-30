<?php

declare(strict_types=1);

namespace Test;

require_once __DIR__ . '/AwaitsForkedChild.php';

use PHPUnit\Framework\Attributes\RequiresPhpExtension;
use PHPUnit\Framework\TestCase;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Wasi;

/** Prefork servers compile modules in the parent and keep using wasm in the workers. */
#[RequiresPhpExtension('pcntl')]
final class ForkTest extends TestCase
{
    use AwaitsForkedChild;

    public function test_a_forked_child_can_compile_and_run_after_the_parent_compiled(): void
    {
        new Module(self::largeModule());

        $pid = pcntl_fork();
        if ($pid === 0) {
            $instance = new Instance(new Module(self::largeModule()));
            exit($instance->exports->f0() === 0 ? 0 : 1);
        }

        $this->assertChildExitsCleanly($pid, 'The forked child did not finish compiling');
    }

    public function test_a_forked_child_can_use_wasi_file_access_after_the_parent_did(): void
    {
        $dir = WasiTest::tempDir();
        file_put_contents("$dir/hello.txt", 'from the host');
        $module = new Module(WasiTest::module(WasiTest::OPEN_AND_READ));
        $run = static function () use ($module, $dir): string {
            $wasi = new Wasi(preopens: ['/data' => $dir]);
            $wasi->start(new Instance($module, $wasi->getImportObject()));

            return $wasi->stdout();
        };
        self::assertSame('from the host', $run());

        $pid = pcntl_fork();
        if ($pid === 0) {
            exit($run() === 'from the host' ? 0 : 1);
        }

        $this->assertChildExitsCleanly($pid, 'The forked child did not finish its WASI run');
    }

    private static function largeModule(): string
    {
        $functions = '';
        for ($i = 0; $i < 200; $i++) {
            $functions .= "(func (export \"f$i\") (result i32) (i32.const 0))\n";
        }

        return "(module $functions)";
    }
}
