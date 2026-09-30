<?php

declare(strict_types=1);

/**
 * DOOM in the terminal, running doom.wasm from PHP.
 *
 *     examples/doom/download.sh
 *     php examples/doom/doom.php
 *
 * Without a terminal, `--ticks=N` runs N game ticks and `--screenshot=file.ppm`
 * writes the last frame, for example in CI.
 */

require __DIR__ . '/Game.php';
require __DIR__ . '/Keyboard.php';
require __DIR__ . '/Renderer.php';
require __DIR__ . '/Terminal.php';

use Example\Doom\Game;
use Example\Doom\Keyboard;
use Example\Doom\Renderer;
use Example\Doom\Terminal;

$options = getopt('', ['ticks:', 'screenshot:']);
$wasm = __DIR__ . '/dist/doom.wasm';
if (!is_file($wasm)) {
    fwrite(STDERR, "doom.wasm is missing, run examples/doom/download.sh first.\n");
    exit(1);
}

$doom = new Game($wasm, __DIR__ . '/dist/saves');

if (isset($options['ticks'])) {
    runHeadless($doom, (int) $options['ticks'], $options['screenshot'] ?? null);
    exit(0);
}

if (!stream_isatty(STDIN) || !stream_isatty(STDOUT)) {
    fwrite(STDERR, "Run this in a terminal, or pass --ticks=N to run without one.\n");
    exit(1);
}

$terminal = new Terminal();
$terminal->enter();
register_shutdown_function($terminal->leave(...));
$keyboard = new Keyboard(doomKeys($doom), enableKittyKeyboard($terminal));

play($doom, $terminal, $keyboard, new Renderer());

function play(Game $doom, Terminal $terminal, Keyboard $keyboard, Renderer $renderer): void
{
    $tickLength = 1 / Game::TICKS_PER_SECOND;
    $nextTick = microtime(true);
    $renderedFrames = 0;
    $fpsSince = microtime(true);
    $fps = 0.0;
    $renderedFrame = -1;

    while (!$keyboard->quit) {
        $now = microtime(true);
        foreach ($keyboard->feed($terminal->read(), $now) as [$pressed, $keys]) {
            foreach ($keys as $key) {
                $pressed ? $doom->press($key) : $doom->release($key);
            }
        }

        $doom->tick();
        $nextTick += $tickLength;

        // Drawing is skipped while the game runs behind, so game speed never depends on the terminal.
        if ($doom->frame !== null && $doom->frames !== $renderedFrame && microtime(true) < $nextTick) {
            [$columns, $rows] = $terminal->size();
            $output = $renderer->render($doom->frame, $doom->width, $doom->height, $columns, $rows);
            $terminal->write($output . statusLine($columns, $rows, $fps, $keyboard));
            $renderedFrame = $doom->frames;
            $renderedFrames++;
        }

        if (microtime(true) - $fpsSince >= 1) {
            $fps = $renderedFrames / (microtime(true) - $fpsSince);
            [$renderedFrames, $fpsSince] = [0, microtime(true)];
        }

        $sleep = $nextTick - microtime(true);
        if ($sleep > 0) {
            usleep((int) ($sleep * 1_000_000));
        } elseif ($sleep < -1) {
            // More than a second behind, for example after the terminal was suspended.
            $nextTick = microtime(true);
        }
    }
}

function statusLine(int $columns, int $rows, float $fps, Keyboard $keyboard): string
{
    $mode = $keyboard->reportsReleases ? 'kitty keys' : 'repeat keys';
    $text = sprintf(' %4.1f fps | %s | arrows/WASD move, space fire, E use, Enter/Esc menu, Ctrl+C quit', $fps, $mode);

    return "\e[{$rows};1H\e[0m\e[2K" . substr($text, 0, $columns);
}

/**
 * Asks for the kitty keyboard protocol with key release events and checks
 * whether the terminal answered, using the device attributes reply that every
 * terminal sends as the end marker.
 */
function enableKittyKeyboard(Terminal $terminal): bool
{
    $terminal->write("\e[?u\e[c");
    $reply = '';
    $deadline = microtime(true) + 0.3;
    while (microtime(true) < $deadline && !preg_match('/\e\[\?[\d;]*c/', $reply)) {
        $reply .= $terminal->read();
        usleep(5_000);
    }
    if (!preg_match('/\e\[\?\d+u/', $reply)) {
        return false;
    }
    // 1: unambiguous escape codes, 2: report press, repeat and release, 8: every key as an escape code.
    $terminal->write("\e[>11u");
    register_shutdown_function(static fn () => $terminal->write("\e[<u"));

    return true;
}

/** @return array<string, int> */
function doomKeys(Game $doom): array
{
    $keys = [];
    foreach (['KEY_UPARROW', 'KEY_DOWNARROW', 'KEY_LEFTARROW', 'KEY_RIGHTARROW', 'KEY_STRAFE_L', 'KEY_STRAFE_R',
        'KEY_FIRE', 'KEY_USE', 'KEY_ENTER', 'KEY_ESCAPE', 'KEY_TAB', 'KEY_BACKSPACE', 'KEY_SHIFT'] as $name) {
        $keys[$name] = $doom->key($name);
    }

    return $keys;
}

function runHeadless(Game $doom, int $ticks, ?string $screenshot): void
{
    $started = hrtime(true);
    for ($i = 0; $i < $ticks; $i++) {
        $doom->tick();
    }
    $milliseconds = (hrtime(true) - $started) / 1e6;

    printf("%d ticks in %.0f ms (%.1f ms per tick), %d frames drawn, %dx%d\n", $ticks, $milliseconds, $milliseconds / max($ticks, 1), $doom->frames, $doom->width, $doom->height);
    foreach ($doom->messages as $message) {
        echo '  ', trim($message), "\n";
    }

    if ($screenshot !== null && $doom->frame !== null) {
        $rgb = '';
        foreach (str_split($doom->frame, 4) as $pixel) {
            $rgb .= $pixel[2] . $pixel[1] . $pixel[0];
        }
        file_put_contents($screenshot, "P6\n{$doom->width} {$doom->height}\n255\n$rgb");
        echo "screenshot written to $screenshot\n";
    }
}
