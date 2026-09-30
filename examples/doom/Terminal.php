<?php

declare(strict_types=1);

namespace Example\Doom;

/** Raw terminal mode, the alternate screen and the terminal size. */
final class Terminal
{
    private ?string $savedMode = null;

    public function enter(): void
    {
        $this->savedMode = trim((string) shell_exec('stty -g < /dev/tty'));
        // No line buffering, no echo, and Ctrl+C arrives as a byte, so the
        // game loop can restore the terminal before it exits.
        shell_exec('stty -icanon -echo -isig min 0 time 0 < /dev/tty');
        stream_set_blocking(STDIN, false);
        // Alternate screen, hidden cursor, cleared.
        $this->write("\e[?1049h\e[?25l\e[2J");
    }

    public function leave(): void
    {
        if ($this->savedMode === null) {
            return;
        }
        $this->write("\e[0m\e[?25h\e[?1049l");
        shell_exec('stty ' . escapeshellarg($this->savedMode) . ' < /dev/tty');
        stream_set_blocking(STDIN, true);
        $this->savedMode = null;
    }

    /** @return array{int, int} columns and rows */
    public function size(): array
    {
        [$rows, $columns] = array_map('intval', explode(' ', trim((string) shell_exec('stty size < /dev/tty'))) + [24, 80]);

        return [max($columns, 20), max($rows, 10)];
    }

    public function read(): string
    {
        return (string) fread(STDIN, 4096);
    }

    public function write(string $output): void
    {
        fwrite(STDOUT, $output);
        fflush(STDOUT);
    }
}
