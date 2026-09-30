<?php

declare(strict_types=1);

namespace Example\Doom;

use Wasm\Exports;
use Wasm\Instance;
use Wasm\Memory;
use Wasm\Module;

/**
 * The game itself: doom.wasm with its ten imports implemented in PHP.
 *
 * The module draws into a BGRA framebuffer in its own memory and calls
 * `ui.drawFrame` with a pointer to it. Time, console messages and save games
 * are imports too, and keys go in through `reportKeyDown` and `reportKeyUp`.
 */
final class Game
{
    public const TICKS_PER_SECOND = 35;

    public readonly Exports $exports;
    public int $width = 0;
    public int $height = 0;
    /** The latest frame as BGRA bytes, or null before the first one. */
    public ?string $frame = null;
    public int $frames = 0;
    /** @var list<string> */
    public array $messages = [];

    public function __construct(string $wasmFile, private readonly string $saveDirectory)
    {
        $instance = null;
        // By reference: the closures run after $instance has been assigned.
        $memory = static function () use (&$instance): Memory {
            return $instance->exports->memory;
        };

        $instance = new Instance(new Module(file_get_contents($wasmFile)), [
            'console' => [
                'onInfoMessage' => fn (int $ptr, int $len) => $this->messages[] = $memory()->read($ptr, $len),
                'onErrorMessage' => fn (int $ptr, int $len) => $this->messages[] = 'error: ' . $memory()->read($ptr, $len),
            ],
            'loading' => [
                'onGameInit' => function (int $width, int $height): void {
                    [$this->width, $this->height] = [$width, $height];
                },
                // Reporting no WADs of our own makes doom.wasm load its embedded
                // shareware DOOM1.WAD, and readWads is then never called.
                'wadSizes' => static function (int $countPtr, int $totalSizePtr): void {
                },
                'readWads' => static function (int $dataPtr, int $sizesPtr): void {
                },
            ],
            'runtimeControl' => [
                'timeInMilliseconds' => static fn (): int => intdiv(hrtime(true), 1_000_000),
            ],
            'ui' => [
                'drawFrame' => function (int $ptr) use ($memory): void {
                    $this->frame = $memory()->read($ptr, $this->width * $this->height * 4);
                    $this->frames++;
                },
            ],
            'gameSaving' => [
                'sizeOfSaveGame' => fn (int $slot): int => is_file($this->saveFile($slot)) ? filesize($this->saveFile($slot)) : 0,
                'readSaveGame' => function (int $slot, int $ptr) use ($memory): int {
                    $data = (string) file_get_contents($this->saveFile($slot));
                    $memory()->write($ptr, $data);

                    return strlen($data);
                },
                'writeSaveGame' => function (int $slot, int $ptr, int $size) use ($memory): int {
                    @mkdir(dirname($this->saveFile($slot)), recursive: true);

                    return (int) file_put_contents($this->saveFile($slot), $memory()->read($ptr, $size));
                },
            ],
        ]);

        $this->exports = $instance->exports;
        $this->exports->initGame();
    }

    public function tick(): void
    {
        $this->exports->tickGame();
    }

    /** @param string $name One of doom.wasm's KEY_* exports, such as KEY_FIRE. */
    public function key(string $name): int
    {
        return $this->exports->{$name}->value;
    }

    public function press(int $key): void
    {
        $this->exports->reportKeyDown($key);
    }

    public function release(int $key): void
    {
        $this->exports->reportKeyUp($key);
    }

    private function saveFile(int $slot): string
    {
        return "{$this->saveDirectory}/doom-save-$slot.dsg";
    }
}
