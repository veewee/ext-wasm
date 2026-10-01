<?php

// Router for the built-in server the HTTP tests start. Every request is
// appended to the log file named in HTTP_TEST_LOG.
file_put_contents((string) getenv('HTTP_TEST_LOG'), $_SERVER['REQUEST_METHOD'] . ' ' . $_SERVER['REQUEST_URI'] . "\n", FILE_APPEND);
if (isset($_GET['sleep'])) {
    sleep((int) $_GET['sleep']);
}
header('Content-Type: text/plain');
echo 'hello from ', $_SERVER['REQUEST_URI'];
