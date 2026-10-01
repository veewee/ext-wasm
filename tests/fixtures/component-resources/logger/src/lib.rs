//! A component that uses the `logger` resource the PHP host implements, for
//! the extension's tests.

wit_bindgen::generate!({ world: "logging", path: "../wit" });


struct Component;

impl Guest for Component {
    fn uses_logger(prefix: String, line: String) -> String {
        Logger::new(&prefix).write(&line)
    }

    fn echo_logger(l: Logger) -> Logger {
        l
    }

    fn borrow_logger(l: &Logger, line: String) -> String {
        l.write(&line)
    }

    fn take_logger(_l: Logger, n: u32) -> u32 {
        n
    }
}

export!(Component);
