//! A component that imports the `counters` interface, for the extension's
//! composition tests: another component instance provides it.

wit_bindgen::generate!({ world: "composing", path: "../wit" });

use docs::demo::counters::total;

struct Component;

impl Guest for Component {
    fn make_and_count(start: u32) -> u32 {
        let counter = Counter::new(start);
        counter.increment();
        counter.increment()
    }

    fn pass_through(c: Counter) -> Counter {
        c.increment();
        c
    }

    fn peek(c: &Counter) -> u32 {
        c.value()
    }

    fn total_of(a: &Counter, b: &Counter) -> u32 {
        total(a, b)
    }
}

export!(Component);
