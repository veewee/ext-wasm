<?php

declare(strict_types=1);

namespace Example\Doom;

/**
 * Draws a BGRA framebuffer with the upper half block character: each terminal
 * cell shows two pixels, the top one as foreground and the bottom one as
 * background colour, both in 24-bit colour.
 *
 * Only cells that changed since the previous frame are written, which keeps
 * the output small enough for the terminal to keep up.
 */
final class Renderer
{
    /** DOOM's 320x200 picture was shown stretched to 4:3 on the monitors of its day. */
    private const ASPECT = 4 / 3;

    /** @var array<int, int> previous colours per cell, top colour in the high bits */
    private array $previous = [];
    private string $layout = '';

    /** @return string the escape sequences that draw `$frame` */
    public function render(string $frame, int $width, int $height, int $columns, int $rows): string
    {
        [$pixelsX, $pixelsY, $left, $top] = $this->fit($columns, $rows);
        $layout = "$columns:$rows";
        if ($layout !== $this->layout) {
            // Resized: forget what is on screen and start from a clear screen.
            $this->layout = $layout;
            $this->previous = [];
            $output = "\e[0m\e[2J";
        } else {
            $output = '';
        }

        $cellRows = intdiv($pixelsY, 2);
        $sourceX = [];
        for ($x = 0; $x < $pixelsX; $x++) {
            $sourceX[$x] = intdiv($x * $width, $pixelsX) * 4;
        }

        for ($row = 0; $row < $cellRows; $row++) {
            $topLine = intdiv(2 * $row * $height, $pixelsY) * $width * 4;
            $bottomLine = intdiv((2 * $row + 1) * $height, $pixelsY) * $width * 4;
            $cursorAt = -1;
            $foreground = $background = -1;

            for ($x = 0; $x < $pixelsX; $x++) {
                $a = $topLine + $sourceX[$x];
                $b = $bottomLine + $sourceX[$x];
                $upper = (ord($frame[$a + 2]) << 16) | (ord($frame[$a + 1]) << 8) | ord($frame[$a]);
                $lower = (ord($frame[$b + 2]) << 16) | (ord($frame[$b + 1]) << 8) | ord($frame[$b]);
                $cell = $row * $pixelsX + $x;
                $colours = ($upper << 24) | $lower;
                if (($this->previous[$cell] ?? -1) === $colours) {
                    continue;
                }
                $this->previous[$cell] = $colours;

                if ($cursorAt !== $x) {
                    $output .= "\e[" . ($top + $row + 1) . ';' . ($left + $x + 1) . 'H';
                }
                if ($upper !== $foreground) {
                    $output .= "\e[38;2;" . ($upper >> 16) . ';' . (($upper >> 8) & 255) . ';' . ($upper & 255) . 'm';
                    $foreground = $upper;
                }
                if ($lower !== $background) {
                    $output .= "\e[48;2;" . ($lower >> 16) . ';' . (($lower >> 8) & 255) . ';' . ($lower & 255) . 'm';
                    $background = $lower;
                }
                $output .= '▀';
                $cursorAt = $x + 1;
            }
        }

        return $output . "\e[0m";
    }

    /**
     * The largest 4:3 picture that fits, leaving the last row for the status line.
     *
     * @return array{int, int, int, int} pixels wide, pixels high, left and top offset in cells
     */
    private function fit(int $columns, int $rows): array
    {
        $maxX = $columns;
        $maxY = ($rows - 1) * 2;
        $pixelsX = min($maxX, (int) floor($maxY * self::ASPECT));
        $pixelsY = min($maxY, (int) floor($pixelsX / self::ASPECT));
        $pixelsY -= $pixelsY % 2;

        return [$pixelsX, $pixelsY, intdiv($columns - $pixelsX, 2), intdiv($rows - 1 - intdiv($pixelsY, 2), 2)];
    }
}
