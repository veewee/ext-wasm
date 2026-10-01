//! `wasm.memory_limit`. PHP calls the handler below whenever the setting
//! changes, so an invalid value is refused the way PHP refuses its own, and a
//! store reads the parsed value without looking the setting up.

use std::cell::Cell;
use std::os::raw::{c_int, c_void};

use ext_php_rs::ffi::{zend_ini_entry, zend_string};
use ext_php_rs::types::ZendStr;
use wasmtime::StoreContextMut;

use crate::store::HostState;

thread_local! {
    static MEMORY_LIMIT: Cell<Option<u64>> = const { Cell::new(None) };
}

/// The bytes of linear memory, tables and GC heap a new store may use.
pub fn memory_limit() -> Option<u64> {
    MEMORY_LIMIT.with(Cell::get)
}

/// Counts the memory of one store against `wasm.memory_limit`.
#[derive(Default)]
pub struct MemoryBudget {
    limit: Option<u64>,
    used: u64,
    /// `used` before the operation that `reset` started, for `undo`.
    before: u64,
    /// Set when the limit refused a growth, so the error can name it.
    refused: bool,
}

/// A table element counts as the size of a pointer, as wasmtime documents.
const TABLE_ELEMENT: u64 = 8;

impl MemoryBudget {
    pub fn new(limit: Option<u64>) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    /// Charges a growth to `desired`. A growth past `maximum` is granted
    /// without charging it, because wasmtime refuses it right after; a
    /// growth that fails for another reason after this stays charged, since
    /// wasmtime reports some failures without asking the limiter first, and
    /// taking back a charge then would undo one that did happen.
    fn grow(&mut self, current: usize, desired: usize, maximum: Option<usize>, unit: u64) -> bool {
        if maximum.is_some_and(|maximum| desired > maximum) {
            return true;
        }
        let bytes = (desired.saturating_sub(current) as u64).saturating_mul(unit);
        let wanted = self.used.saturating_add(bytes);
        if self.limit.is_some_and(|limit| wanted > limit) {
            self.refused = true;
            return false;
        }
        self.used = wanted;
        true
    }

    /// Gives back what the operation since `reset` charged, after it failed
    /// and freed its memory again.
    pub fn undo(&mut self) {
        self.used = self.before;
    }

    /// Replaces `err` with one naming the limit when the limit caused it.
    pub fn explain(&mut self, err: wasmtime::Error) -> wasmtime::Error {
        match (std::mem::take(&mut self.refused), self.limit) {
            (true, Some(limit)) => wasmtime::Error::msg(format!(
                "the store's memory would exceed wasm.memory_limit of {limit} bytes"
            )),
            _ => err,
        }
    }

    /// Starts an operation: forgets an earlier refusal and remembers the
    /// budget for `undo`.
    pub fn reset(&mut self) {
        self.refused = false;
        self.before = self.used;
    }
}

impl wasmtime::ResourceLimiter for MemoryBudget {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(self.grow(current, desired, maximum, 1))
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(self.grow(current, desired, maximum, TABLE_ELEMENT))
    }
}

/// Runs `f`, which may grow the store's memory, and names wasm.memory_limit
/// in its error when the limit refused a growth.
pub fn limited<T>(
    ctx: &mut StoreContextMut<'_, HostState>,
    f: impl FnOnce(&mut StoreContextMut<'_, HostState>) -> wasmtime::Result<T>,
) -> wasmtime::Result<T> {
    ctx.data_mut().memory.reset();
    f(ctx).map_err(|err| {
        let memory = &mut ctx.data_mut().memory;
        memory.undo();
        memory.explain(err)
    })
}

const SUCCESS: c_int = 0;
const FAILURE: c_int = -1;

/// # Safety
/// Called by PHP with the new value of the setting.
pub unsafe extern "C" fn on_memory_limit(
    _entry: *mut zend_ini_entry,
    new_value: *mut zend_string,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _stage: c_int,
) -> c_int {
    let value = if new_value.is_null() {
        Some("")
    } else {
        let value: &ZendStr = unsafe { &*new_value.cast::<ZendStr>() };
        value.as_str().ok()
    };
    match value.map(str::trim).and_then(parse_memory) {
        Some(limit) => {
            MEMORY_LIMIT.with(|current| current.set(limit));
            SUCCESS
        }
        None => FAILURE,
    }
}

/// Bytes with an optional K, M or G suffix, as PHP's `memory_limit`. `0` and
/// `-1` mean no limit. `None` for an invalid value.
fn parse_memory(value: &str) -> Option<Option<u64>> {
    if value.is_empty() || value == "-1" {
        return Some(None);
    }
    let (digits, unit) = match value.char_indices().last() {
        Some((at, 'k' | 'K')) => (&value[..at], 1 << 10),
        Some((at, 'm' | 'M')) => (&value[..at], 1 << 20),
        Some((at, 'g' | 'G')) => (&value[..at], 1 << 30),
        _ => (value, 1),
    };
    let bytes = digits.parse::<u64>().ok()?.checked_mul(unit)?;
    Some((bytes > 0).then_some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_memory_limits() {
        assert_eq!(parse_memory("0"), Some(None));
        assert_eq!(parse_memory("-1"), Some(None));
        assert_eq!(parse_memory("512"), Some(Some(512)));
        assert_eq!(parse_memory("64M"), Some(Some(64 << 20)));
        assert_eq!(parse_memory("2g"), Some(Some(2 << 30)));
        assert_eq!(parse_memory("12X"), None);
        assert_eq!(parse_memory("lots"), None);
        assert_eq!(parse_memory("-5"), None);
    }
}
