<?php

declare(strict_types=1);

namespace Example\Doom;

/**
 * Turns terminal input into DOOM key presses and releases.
 *
 * DOOM needs to know when a key goes up. Terminals that speak the kitty
 * keyboard protocol report that. Others only send a key again while it
 * auto-repeats, so a key counts as released once its repeats stop.
 */
final class Keyboard
{
    /** Seconds before a key that never repeated counts as released: longer than a typical repeat delay. */
    private const FIRST_RELEASE_AFTER = 0.55;
    /** Seconds without a repeat before a repeating key counts as released. */
    private const REPEAT_RELEASE_AFTER = 0.12;

    public bool $quit = false;
    /** @var array<string, array{keys: list<int>, lastSeen: float, repeating: bool}> */
    private array $held = [];

    /** @param array<string, int> $doomKeys doom.wasm's KEY_* values by name */
    public function __construct(private readonly array $doomKeys, public readonly bool $reportsReleases)
    {
    }

    /**
     * @return list<array{bool, list<int>}> events as [pressed, doom keys]
     */
    public function feed(string $input, float $now): array
    {
        $events = [];
        foreach ($this->reportsReleases ? $this->kittyKeys($input) : $this->legacyKeys($input) as [$name, $kind]) {
            $keys = $this->doomKeysFor($name);
            if ($keys === []) {
                continue;
            }
            if ($kind === 'release') {
                if (isset($this->held[$name])) {
                    unset($this->held[$name]);
                    $events[] = [false, $keys];
                }
                continue;
            }
            if (isset($this->held[$name])) {
                $this->held[$name]['lastSeen'] = $now;
                $this->held[$name]['repeating'] = true;
                continue;
            }
            $this->held[$name] = ['keys' => $keys, 'lastSeen' => $now, 'repeating' => false];
            $events[] = [true, $keys];
        }

        if (!$this->reportsReleases) {
            foreach ($this->held as $name => $key) {
                $timeout = $key['repeating'] ? self::REPEAT_RELEASE_AFTER : self::FIRST_RELEASE_AFTER;
                if ($now - $key['lastSeen'] > $timeout) {
                    unset($this->held[$name]);
                    $events[] = [false, $key['keys']];
                }
            }
        }

        return $events;
    }

    /** Letters used for movement also go through as themselves, so cheat codes still work. */
    private function doomKeysFor(string $name): array
    {
        $binding = [
            'up' => 'KEY_UPARROW', 'w' => 'KEY_UPARROW',
            'down' => 'KEY_DOWNARROW', 's' => 'KEY_DOWNARROW',
            'left' => 'KEY_LEFTARROW', 'right' => 'KEY_RIGHTARROW',
            'a' => 'KEY_STRAFE_L', 'd' => 'KEY_STRAFE_R',
            'space' => 'KEY_FIRE', 'e' => 'KEY_USE',
            'enter' => 'KEY_ENTER', 'escape' => 'KEY_ESCAPE', 'tab' => 'KEY_TAB',
            'backspace' => 'KEY_BACKSPACE', 'shift' => 'KEY_SHIFT',
        ][$name] ?? null;

        $keys = $binding !== null ? [$this->doomKeys[$binding]] : [];
        if (strlen($name) === 1) {
            $keys[] = ord($name);
        }

        return $keys;
    }

    /** @return list<array{string, string}> [key name, 'press'] for classic terminal input */
    private function legacyKeys(string $input): array
    {
        $keys = [];
        $arrows = ['A' => 'up', 'B' => 'down', 'C' => 'right', 'D' => 'left'];
        for ($i = 0, $length = strlen($input); $i < $length; $i++) {
            $byte = $input[$i];
            if ($byte === "\e") {
                $next = $input[$i + 1] ?? '';
                if (($next === '[' || $next === 'O') && isset($arrows[$input[$i + 2] ?? ''])) {
                    $keys[] = [$arrows[$input[$i + 2]], 'press'];
                    $i += 2;
                } else {
                    $keys[] = ['escape', 'press'];
                }
                continue;
            }
            $name = match ($byte) {
                "\x03" => null,
                "\r", "\n" => 'enter',
                "\t" => 'tab',
                ' ' => 'space',
                "\x7f", "\x08" => 'backspace',
                default => ctype_print($byte) ? strtolower($byte) : null,
            };
            if ($byte === "\x03") {
                $this->quit = true;
            }
            if ($name !== null) {
                $keys[] = [$name, 'press'];
            }
        }

        return $keys;
    }

    /**
     * Parses kitty keyboard protocol events: CSI code[;modifiers[:event]] u, or
     * with a letter instead of u for arrows. Event 1 is a press, 2 a repeat, 3 a release.
     * An arrow press without modifiers comes without numbers at all, as CSI A.
     *
     * @return list<array{string, string}>
     */
    private function kittyKeys(string $input): array
    {
        preg_match_all('/\e\[(\d*)(?::\d*)*(?:;(\d+)(?::(\d+))?)?([uABCD~])/', $input, $matches, PREG_SET_ORDER);
        $keys = [];
        foreach ($matches as $match) {
            [, $code, $modifiers, $event, $final] = $match + ['', '', '1', '1', ''];
            $ctrl = ((int) ($modifiers ?: 1) - 1) & 4;
            $name = match (true) {
                $final === 'A' => 'up',
                $final === 'B' => 'down',
                $final === 'C' => 'right',
                $final === 'D' => 'left',
                $final !== 'u' => null,
                $code === '13' => 'enter',
                $code === '27' => 'escape',
                $code === '9' => 'tab',
                $code === '32' => 'space',
                $code === '127' => 'backspace',
                in_array($code, ['57441', '57447'], true) => 'shift',
                (int) $code < 128 && ctype_print(chr((int) $code)) => strtolower(chr((int) $code)),
                default => null,
            };
            if ($ctrl && $name === 'c') {
                $this->quit = true;
                continue;
            }
            if ($name === null || $event === '2') {
                continue;
            }
            $keys[] = [$name, $event === '3' ? 'release' : 'press'];
        }

        return $keys;
    }
}
