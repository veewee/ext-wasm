<?php

declare(strict_types=1);

namespace Test;

/**
 * Waits for a forked child with a deadline, so a child that hangs fails the
 * test instead of blocking the whole run.
 */
trait AwaitsForkedChild
{
    private function assertChildExitsCleanly(int $pid, string $failure): void
    {
        $deadline = microtime(true) + 20;
        do {
            if (pcntl_waitpid($pid, $status, WNOHANG) === $pid) {
                self::assertSame(0, pcntl_wexitstatus($status));

                return;
            }
            usleep(50_000);
        } while (microtime(true) < $deadline);

        posix_kill($pid, SIGKILL);
        pcntl_waitpid($pid, $status);
        self::fail("$failure within 20 seconds");
    }
}
