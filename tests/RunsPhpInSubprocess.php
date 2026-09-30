<?php

declare(strict_types=1);

namespace Test;

/**
 * Runs a PHP script in a fresh process with the extension under test.
 *
 * Needed for behaviour that depends on the whole process: a crash, or whether
 * the store is freed, which any wasm object left over from another test prevents.
 */
trait RunsPhpInSubprocess
{
    /** @param array<string, string> $settings extra php.ini settings for the child */
    private function runPhp(string $code, ?int &$exitCode = null, array $settings = []): string
    {
        $file = tempnam(sys_get_temp_dir(), 'wasm-test');
        file_put_contents($file, $code);
        $command = [PHP_BINARY, '-n', '-d', 'extension=' . self::extensionUnderTest()];
        foreach ($settings as $name => $value) {
            // Quoted, because INI syntax gives characters such as ~ in Windows
            // short paths a meaning of their own.
            array_push($command, '-d', $name . '="' . addcslashes($value, '"\\') . '"');
        }
        $command[] = $file;
        $process = proc_open($command, [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
        $output = stream_get_contents($pipes[1]) . stream_get_contents($pipes[2]);
        $exitCode = proc_close($process);
        unlink($file);

        return trim($output);
    }

    private static function extensionUnderTest(): string
    {
        $override = getenv('WASM_EXTENSION');
        if (is_string($override) && $override !== '') {
            return $override;
        }
        $candidates = glob(dirname(__DIR__) . '/target/{release,debug}/{libwasm.so,libwasm.dylib,wasm.dll}', GLOB_BRACE) ?: [];
        usort($candidates, fn (string $a, string $b): int => filemtime($b) <=> filemtime($a));

        return $candidates[0] ?? 'wasm';
    }
}
