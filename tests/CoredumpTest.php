<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;

/** wasm.coredump_dir: a wasm coredump file for each trap. */
final class CoredumpTest extends TestCase
{
    use RunsPhpInSubprocess;

    /** Stores the marker PHPMARK! at address 16 while running, then traps. */
    private const TRAPPING_MODULE = <<<'PHP'
        $module = new Wasm\Module('(module
          (memory (export "memory") 1)
          (func (export "crash")
            (i64.store (i32.const 16) (i64.const 0x214b52414d504850))
            unreachable))');
        PHP;

    private string $directory;

    protected function setUp(): void
    {
        $this->directory = sys_get_temp_dir() . '/wasm-coredump-' . bin2hex(random_bytes(6));
        mkdir($this->directory);
    }

    protected function tearDown(): void
    {
        foreach (glob($this->directory . '/*') ?: [] as $file) {
            unlink($file);
        }
        rmdir($this->directory);
    }

    private function runWith(string $code, string $directory): string
    {
        return $this->runPhp("<?php\n" . $code, settings: ['wasm.coredump_dir' => $directory]);
    }

    /** @return list<string> */
    private function dumps(): array
    {
        return glob($this->directory . '/*.coredump') ?: [];
    }

    private const CATCH = <<<'PHP'
        try {
            %s;
        } catch (Throwable $e) {
            echo get_class($e), ': ', $e->getMessage();
        }
        PHP;

    private function crashModule(): string
    {
        return self::TRAPPING_MODULE . sprintf(self::CATCH, '(new Wasm\Instance($module))->exports->crash()');
    }

    public function test_without_the_setting_a_trap_writes_nothing(): void
    {
        $output = $this->runPhp("<?php\n" . $this->crashModule());

        self::assertStringStartsWith("Wasm\\Exception\\RuntimeError: wasm trap: wasm `unreachable` instruction executed\n", $output);
        self::assertStringNotContainsString('coredump', $output);
    }

    public function test_a_trap_writes_a_coredump_with_the_memory_at_the_time_of_the_trap(): void
    {
        $without = $this->runPhp("<?php\n" . $this->crashModule());
        $output = $this->runWith($this->crashModule(), $this->directory);

        $dumps = $this->dumps();
        self::assertCount(1, $dumps);
        self::assertSame($without . "\ncoredump: " . $dumps[0], $output);
        self::assertMatchesRegularExpression('/\/wasm-\d+-\d+-\d+\.coredump$/', $dumps[0]);
        $bytes = file_get_contents($dumps[0]);
        self::assertStringStartsWith("\0asm", $bytes);
        self::assertStringContainsString('corestack', $bytes);
        self::assertStringContainsString('PHPMARK!', $bytes);
        if (PHP_OS_FAMILY !== 'Windows') {
            self::assertSame(0600, fileperms($dumps[0]) & 0777);
        }
    }

    public function test_each_trap_writes_its_own_file(): void
    {
        $this->runWith($this->crashModule() . $this->crashModule(), $this->directory);

        self::assertCount(2, $this->dumps());
    }

    public function test_a_trap_in_a_component_writes_a_coredump(): void
    {
        $output = $this->runWith(<<<'PHP'
            $component = new Wasm\Component\Component('(component
              (core module $m (func (export "crash") unreachable))
              (core instance $i (instantiate $m))
              (func (export "crash") (canon lift (core func $i "crash"))))');
            $exports = (new Wasm\Component\Instance($component))->exports;
            PHP . sprintf(self::CATCH, '$exports->crash()') . "\necho \"\\n---\\n\";\n" . sprintf(self::CATCH, '$exports->crash()'), $this->directory);

        self::assertCount(1, $this->dumps(), 'the second call cannot enter the instance, so it runs no wasm');
        [$first, $second] = explode("\n---\n", $output);
        self::assertStringEndsWith("\ncoredump: " . $this->dumps()[0], $first);
        self::assertStringNotContainsString('coredump', $second);
    }

    public function test_a_trap_in_an_async_export_writes_a_coredump(): void
    {
        $output = $this->runWith(<<<'PHP'
            $component = new Wasm\Component\Component('(component
              (core module $m
                (func (export "run") (result i32) unreachable)
                (func (export "cb") (param i32 i32 i32) (result i32) unreachable))
              (core instance $i (instantiate $m))
              (func (export "run") async (result u32) (canon lift (core func $i "run") async (callback (core func $i "cb")))))');
            PHP . sprintf(self::CATCH, '(new Wasm\Component\Instance($component))->exports->run()'), $this->directory);

        self::assertCount(1, $this->dumps(), $output);
        self::assertStringEndsWith("\ncoredump: " . $this->dumps()[0], $output);
    }

    public function test_a_trap_in_a_task_writing_a_stream_writes_a_coredump(): void
    {
        $fixture = var_export(__DIR__ . '/fixtures/component-async/component-async.wasm', true);
        $output = $this->runWith(<<<PHP
            \$exports = (new Wasm\\Component\\Instance(
                Wasm\\Component\\Component::fromFile($fixture),
                ['slow' => fn (int \$n): int => \$n],
                new Wasm\\Wasi(),
            ))->exports;
            \$stream = \$exports->crashLater();
            PHP . sprintf(self::CATCH, 'while ($stream->read() !== null) {}'), $this->directory);

        self::assertCount(1, $this->dumps(), $output);
        self::assertStringStartsWith('Wasm\Exception\RuntimeError: ', $output);
        self::assertStringEndsWith("\ncoredump: " . $this->dumps()[0], $output);
    }

    public function test_an_import_that_throws_writes_nothing(): void
    {
        $code = <<<'PHP'
            $module = new Wasm\Module('(module (import "env" "f" (func $f)) (func (export "run") (call $f)))');
            $exports = (new Wasm\Instance($module, ['env' => ['f' => fn () => throw new DomainException('from php')]]))->exports;
            PHP . sprintf(self::CATCH, '$exports->run()');

        self::assertSame('DomainException: from php', $this->runWith($code, $this->directory));
        self::assertCount(0, $this->dumps());
    }

    public function test_a_start_function_whose_import_fails_keeps_its_link_error_message(): void
    {
        $code = <<<'PHP'
            $module = new Wasm\Module('(module (import "env" "f" (func $f (result i32))) (func $start (drop (call $f))) (start $start))');
            PHP . sprintf(self::CATCH, 'new Wasm\Instance($module, [\'env\' => [\'f\' => fn () => \'not an int\']])');

        $without = $this->runPhp("<?php\n" . $code);

        self::assertSame($without, $this->runWith($code, $this->directory));
        self::assertCount(0, $this->dumps());
    }

    public function test_a_directory_that_does_not_exist_is_named_in_the_message(): void
    {
        $output = $this->runWith($this->crashModule(), $this->directory . '/missing');

        self::assertStringStartsWith("Wasm\\Exception\\RuntimeError: wasm trap: wasm `unreachable` instruction executed\n", $output);
        self::assertMatchesRegularExpression('/\ncoredump not written: .+$/', $output);
    }

    public function test_a_relative_directory_is_refused(): void
    {
        $output = $this->runWith($this->crashModule(), 'dumps');

        self::assertStringEndsWith("\ncoredump not written: wasm.coredump_dir must be an absolute path", $output);
    }
}
