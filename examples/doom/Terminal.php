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
        $this->savedMode = null;
    }

    /** @return array{int, int} columns and rows */
    public function size(): array
    {
        [$rows, $columns] = array_map('intval', explode(' ', trim((string) shell_exec('stty size < /dev/tty'))) + [24, 80]);

        return [max($columns, 20), max($rows, 10)];
    }

    /**
     * Returns the input that is waiting, without blocking.
     *
     * STDIN stays in blocking mode: it shares the terminal with STDOUT, and a
     * non-blocking terminal made large frame writes stop after a few KB.
     */
    public function read(): string
    {
        $input = '';
        $read = [STDIN];
        $none = null;
        while (stream_select($read, $none, $none, 0) > 0) {
            $chunk = (string) fread(STDIN, 4096);
            if ($chunk === '') {
                break;
            }
            $input .= $chunk;
            $read = [STDIN];
        }

        return $input;
    }

    public function write(string $output): void
    {
        // A terminal can accept less than asked, so keep writing until all of it is out.
        while ($output !== '') {
            $written = fwrite(STDOUT, $output);
            if ($written === false || $written === 0) {
                return;
            }
            $output = substr($output, $written);
        }
        fflush(STDOUT);
    }
}
