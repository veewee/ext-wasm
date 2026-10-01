<?php

declare(strict_types=1);

namespace Test;

require_once __DIR__ . '/AwaitsForkedChild.php';
require_once __DIR__ . '/RunsPhpInSubprocess.php';
require_once __DIR__ . '/WasiTest.php';

use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\RequiresPhpExtension;
use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Exception\CompileError;
use Wasm\Instance;
use Wasm\Module;
use Wasm\Serializer;
use Wasm\Wasi;

final class SerializerTest extends TestCase
{
    use AwaitsForkedChild;
    use RunsPhpInSubprocess;

    private const MODULE = <<<'WAT'
        (module
          (@custom "meta" "da\00ta")
          (import "env" "log" (func (param i32)))
          (func (export "seven") (result i32) (i32.const 7))
          (memory (export "memory") 1))
        WAT;

    private const COMPONENT = <<<'WAT'
        (component
          (core module $m (func (export "f") (result i32) (i32.const 7)))
          (core instance $i (instantiate $m))
          (func (export "seven") (result u32) (canon lift (core func $i "f"))))
        WAT;

    private const WASI_COMPONENT = __DIR__ . '/fixtures/component-wasi/component-wasi.wasm';

    private static function instantiate(Module $module): Instance
    {
        return new Instance($module, ['env' => ['log' => static function (int $value): void {}]]);
    }

    public function test_a_deserialized_module_runs_and_reflects_like_the_compiled_one(): void
    {
        $compiled = new Module(self::MODULE);
        $serializer = new Serializer();

        $module = $serializer->deserializeModule($serializer->serializeModule($compiled));

        self::assertSame(7, self::instantiate($module)->exports->seven());
        self::assertSame($compiled->exports(), $module->exports());
        self::assertSame($compiled->imports(), $module->imports());
        self::assertSame(["da\0ta"], $module->customSections('meta'));
    }

    public function test_a_deserialized_component_runs_and_reflects_like_the_compiled_one(): void
    {
        $compiled = new Component(self::COMPONENT);
        $serializer = new Serializer();

        $component = $serializer->deserializeComponent($serializer->serializeComponent($compiled));

        self::assertSame(7, (new \Wasm\Component\Instance($component))->exports->seven());
        self::assertEquals($compiled->exports(), $component->exports());
    }

    public function test_a_deserialized_wasi_component_runs(): void
    {
        $serializer = new Serializer();
        $artifact = $serializer->serializeComponent(Component::fromFile(self::WASI_COMPONENT));

        $wasi = new Wasi(args: ['app']);
        $exitCode = $wasi->start(new \Wasm\Component\Instance($serializer->deserializeComponent($artifact), wasi: $wasi));

        self::assertSame(0, $exitCode);
    }

    public function test_artifacts_are_read_from_files(): void
    {
        $dir = WasiTest::tempDir();
        $serializer = new Serializer();
        file_put_contents("$dir/module.cwasm", $serializer->serializeModule(new Module(self::MODULE)));
        file_put_contents("$dir/component.cwasm", $serializer->serializeComponent(new Component(self::COMPONENT)));

        self::assertSame(7, self::instantiate($serializer->deserializeModuleFile("$dir/module.cwasm"))->exports->seven());
        $component = $serializer->deserializeComponentFile("$dir/component.cwasm");
        self::assertSame(7, (new \Wasm\Component\Instance($component))->exports->seven());
    }

    public function test_reading_an_artifact_file_respects_open_basedir(): void
    {
        $dir = WasiTest::tempDir();
        mkdir("$dir/allowed");
        $serializer = new Serializer();
        $module = $serializer->serializeModule(new Module('(module)'));
        $component = $serializer->serializeComponent(new Component(self::COMPONENT));
        file_put_contents("$dir/outside.cwasm", $module);
        file_put_contents("$dir/allowed/inside.cwasm", $module);
        file_put_contents("$dir/outside-component.cwasm", $component);
        file_put_contents("$dir/allowed/inside-component.cwasm", $component);

        $code = <<<'PHP'
            <?php
            $serializer = new Wasm\Serializer();
            $serializer->deserializeModuleFile('DIR/allowed/inside.cwasm');
            $serializer->deserializeComponentFile('DIR/allowed/inside-component.cwasm');
            echo "inside ok\n";
            foreach (['deserializeModuleFile' => 'outside', 'deserializeComponentFile' => 'outside-component'] as $method => $file) {
                try {
                    $serializer->$method("DIR/$file.cwasm");
                    echo "$file read\n";
                } catch (Wasm\Exception\WasmException $e) {
                    echo get_class($e), ': ', $e->getMessage(), "\n";
                }
            }
            PHP;
        $output = $this->runPhp(str_replace('DIR', $dir, $code), settings: ['open_basedir' => "$dir/allowed"]);

        self::assertStringContainsString('inside ok', $output);
        self::assertMatchesRegularExpression('/WasmException: cannot read .*outside\.cwasm.*open_basedir/', $output);
        self::assertMatchesRegularExpression('/WasmException: cannot read .*outside-component\.cwasm.*open_basedir/', $output);
    }

    /** @return iterable<string, array{string, string}> */
    public static function notAnArtifact(): iterable
    {
        $artifact = (new Serializer())->serializeModule(new Module(self::MODULE));
        $payload = strpos($artifact, "\x7fELF");

        yield 'empty string' => ['', 'not a precompiled artifact'];
        yield 'wasm binary' => ["\0asm\x01\0\0\0", 'not a precompiled artifact'];
        yield 'wat text' => ['(module)', 'not a precompiled artifact'];
        yield 'truncated envelope' => [substr($artifact, 0, 12), 'truncated'];
        yield 'truncated payload' => [substr($artifact, 0, $payload + 100), 'corrupted'];
        yield 'changed payload' => [self::flip($artifact, $payload + 200), 'corrupted'];
        yield 'changed custom section' => [self::flip($artifact, strpos($artifact, "da\0ta")), 'corrupted'];
        yield 'wasmtime artifact' => [substr($artifact, $payload), 'not a precompiled artifact from Wasm\Serializer'];
    }

    #[DataProvider('notAnArtifact')]
    public function test_anything_but_an_intact_artifact_is_a_compile_error(string $bytes, string $message): void
    {
        $this->expectException(CompileError::class);
        $this->expectExceptionMessage($message);

        (new Serializer())->deserializeModule($bytes);
    }

    public function test_anything_but_an_intact_component_artifact_is_a_compile_error(): void
    {
        $artifact = (new Serializer())->serializeComponent(new Component(self::COMPONENT));

        $this->expectException(CompileError::class);
        $this->expectExceptionMessage('corrupted');

        (new Serializer())->deserializeComponent(self::flip($artifact, strlen($artifact) - 100));
    }

    public function test_a_module_artifact_is_not_a_component(): void
    {
        $serializer = new Serializer();
        $artifact = $serializer->serializeModule(new Module(self::MODULE));

        $this->expectException(CompileError::class);
        $this->expectExceptionMessage('use Wasm\Serializer::deserializeModule()');

        $serializer->deserializeComponent($artifact);
    }

    public function test_a_component_artifact_is_not_a_module(): void
    {
        $serializer = new Serializer();
        $artifact = $serializer->serializeComponent(new Component(self::COMPONENT));

        $this->expectException(CompileError::class);
        $this->expectExceptionMessage('use Wasm\Serializer::deserializeComponent()');

        $serializer->deserializeModule($artifact);
    }

    public function test_wasmtime_rejects_an_artifact_that_is_not_its_own(): void
    {
        $artifact = (new Serializer())->serializeModule(new Module(self::MODULE));
        $payload = strpos($artifact, "\x7fELF");
        $notElf = self::withCrc(self::flip($artifact, $payload, "\0"));

        $output = $this->runPhp(self::deserializeScript($notElf));

        self::assertStringContainsString('CompileError', $output);
        self::assertStringContainsString('failed to parse precompiled artifact as an ELF', $output);
        self::assertStringContainsString('rebuild the artifact', $output);
    }

    public function test_wasmtime_rejects_an_artifact_of_another_major_version(): void
    {
        $artifact = (new Serializer())->serializeModule(new Module(self::MODULE));
        $payload = strpos($artifact, "\x7fELF");
        $elf = substr($artifact, $payload);
        [$offset] = self::elfSection($elf, '.wasmtime.engine');
        $version = (string) substr($elf, $offset + 2, 2);
        self::assertMatchesRegularExpression('/^\d\d$/', $version, 'the engine section starts with the major version');
        $other = self::withCrc(substr_replace($artifact, '00', $payload + $offset + 2, 2));

        $output = $this->runPhp(self::deserializeScript($other));

        self::assertStringContainsString('CompileError', $output);
        self::assertStringContainsString("incompatible version '00'", $output);
    }

    #[RequiresPhpExtension('pcntl')]
    public function test_a_deserialized_module_works_in_a_forked_child(): void
    {
        $serializer = new Serializer();
        $artifact = $serializer->serializeModule(new Module(self::MODULE));
        self::assertSame(7, self::instantiate($serializer->deserializeModule($artifact))->exports->seven());

        $pid = pcntl_fork();
        if ($pid === 0) {
            exit(self::instantiate($serializer->deserializeModule($artifact))->exports->seven() === 7 ? 0 : 1);
        }

        $this->assertChildExitsCleanly($pid, 'The forked child did not load the artifact');
    }

    public function test_php_serialize_stays_unsupported(): void
    {
        $this->expectException(\Exception::class);
        $this->expectExceptionMessage("Serialization of 'Wasm\\Module' is not allowed");

        serialize(new Module(self::MODULE));
    }

    private static function deserializeScript(string $artifact): string
    {
        return '<?php try { (new Wasm\Serializer())->deserializeModule(base64_decode(\'' . base64_encode($artifact)
            . '\')); echo "loaded"; } catch (Wasm\Exception\CompileError $e) { echo get_class($e), ": ", $e->getMessage(); }';
    }

    private static function flip(string $bytes, int $offset, ?string $replacement = null): string
    {
        $bytes[$offset] = $replacement ?? chr(ord($bytes[$offset]) ^ 0xff);

        return $bytes;
    }

    /** Recomputes the envelope's CRC32 of everything after it; it follows the magic, version and kind bytes. */
    private static function withCrc(string $artifact): string
    {
        return substr_replace($artifact, pack('V', crc32(substr($artifact, 14))), 10, 4);
    }

    /** @return array{int, int} offset and size of a section in a 64-bit little-endian ELF */
    private static function elfSection(string $elf, string $name): array
    {
        $header = unpack('Pshoff', $elf, 40) + unpack('vshentsize/vshnum/vshstrndx', $elf, 58);
        $section = static fn (int $index): array => unpack('Vname/@24/Poffset/Psize', $elf, $header['shoff'] + $index * $header['shentsize']);
        $names = $section($header['shstrndx']);
        for ($i = 0; $i < $header['shnum']; $i++) {
            $candidate = $section($i);
            $start = $names['offset'] + $candidate['name'];
            if (substr($elf, $start, strlen($name) + 1) === "$name\0") {
                return [$candidate['offset'], $candidate['size']];
            }
        }
        self::fail("no $name section");
    }
}
