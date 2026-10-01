//! A component that defines the `counter` resource, for the extension's tests.

use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};

wit_bindgen::generate!({ world: "counting", path: "../wit" });

use exports::docs::demo::counters::{Counter as Handle, CounterBorrow, Guest, GuestCounter};

static DROPPED: AtomicU32 = AtomicU32::new(0);

struct Component;

struct Counter(Cell<u32>);

impl Drop for Counter {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

impl GuestCounter for Counter {
    fn new(start: u32) -> Self {
        Self(Cell::new(start))
    }

    fn increment(&self) -> u32 {
        self.0.set(self.0.get() + 1);
        self.0.get()
    }

    fn value(&self) -> u32 {
        self.0.get()
    }

    fn zero() -> Handle {
        Handle::new(Counter::new(0))
    }
}

impl Guest for Component {
    type Counter = Counter;

    fn total(a: CounterBorrow<'_>, b: CounterBorrow<'_>) -> u32 {
        a.get::<Counter>().value() + b.get::<Counter>().value()
    }

    fn consume(c: Handle) -> u32 {
        c.get::<Counter>().value()
    }

    fn dropped() -> u32 {
        DROPPED.load(Ordering::SeqCst)
    }
}

export!(Component);
