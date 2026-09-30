<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Instance;
use Wasm\Memory;
use Wasm\Module;

final class MemoryTest extends TestCase
{
    private const PAGE = 65536;

    public function test_it_creates_standalone_memory(): void
    {
        $memory = new Memory(['initial' => 1, 'maximum' => 3]);

        self::assertSame(self::PAGE, $memory->byteLength());
        self::assertSame(str_repeat("\0", 4), $memory->read(0, 4));
    }

    public function test_it_reads_and_writes_bytes(): void
    {
        $memory = new Memory(['initial' => 1]);
        $memory->write(10, "hello\0world");

        self::assertSame("hello\0world", $memory->read(10, 11));
        self::assertSame(self::PAGE, strlen($memory->buffer()));
        self::assertSame('hello', substr($memory->buffer(), 10, 5));
    }

    public function test_it_grows(): void
    {
        $memory = new Memory(['initial' => 1, 'maximum' => 2]);

        self::assertSame(1, $memory->grow(1));
        self::assertSame(2 * self::PAGE, $memory->byteLength());
    }

    public function test_growing_past_maximum_throws(): void
    {
        $memory = new Memory(['initial' => 1, 'maximum' => 1]);

        $this->expectException(\ValueError::class);
        $memory->grow(1);
    }

    public function test_out_of_bounds_access_throws(): void
    {
        $memory = new Memory(['initial' => 1]);

        $this->expectException(\ValueError::class);
        $memory->read(self::PAGE - 2, 4);
    }

    public function test_huge_read_length_throws_instead_of_allocating(): void
    {
        $memory = new Memory(['initial' => 1]);

        $this->expectException(\ValueError::class);
        $memory->read(0, 1 << 46);
    }

    public function test_out_of_bounds_write_throws(): void
    {
        $memory = new Memory(['initial' => 1]);

        $this->expectException(\ValueError::class);
        $memory->write(self::PAGE, 'x');
    }

    public function test_initial_is_required(): void
    {
        $this->expectException(\TypeError::class);
        new Memory([]);
    }

    public function test_exported_memory_is_shared_with_wasm(): void
    {
        $exports = (new Instance(new Module(<<<'EOWAT'
            (module
              (memory (export "memory") 1)
              (data (i32.const 0) "abc")
              (func (export "load") (param i32) (result i32) (i32.load8_u (local.get 0))))
            EOWAT)))->exports;

        self::assertInstanceOf(Memory::class, $exports->memory);
        self::assertSame('abc', $exports->memory->read(0, 3));

        $exports->memory->write(0, 'z');
        self::assertSame(ord('z'), $exports->load(0));
    }
}
