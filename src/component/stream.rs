//! `stream<T>` values a component hands to PHP, read chunk by chunk.
//!
//! wasmtime moves stream items only inside its concurrent event loop, so a
//! read runs that loop until the next chunk is there. The items go through
//! a consumer that takes them only while PHP is reading, which keeps an
//! endless stream from filling memory between reads.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpResult;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use ext_php_rs::zend::ce;
use wasmtime::StoreContextMut;
use wasmtime::component::types::Type;
use wasmtime::component::{Source, StreamAny, StreamConsumer, StreamResult};

use crate::error::runtime_error;
use crate::store::{self, HostState, SharedStore};
use crate::value::ConvertError;

/// The most items one read takes from the component.
const CHUNK: usize = 64 * 1024;

/// Converts the items of one chunk to PHP.
trait Item: Sized + Send + 'static {
    fn to_zval(self) -> Zval;

    fn chunk(items: Vec<Self>) -> Zval {
        let mut list = ZendHashTable::new();
        for item in items {
            let _ = list.push(item.to_zval());
        }
        let mut zval = Zval::new();
        zval.set_hashtable(list);
        zval
    }
}

macro_rules! long_items {
    ($($ty:ty),*) => {$(
        impl Item for $ty {
            fn to_zval(self) -> Zval {
                let mut zval = Zval::new();
                zval.set_long(self as i64);
                zval
            }
        }
    )*};
}

long_items!(i8, i16, u16, i32, u32, i64);

impl Item for u64 {
    /// Keeps its bits above `PHP_INT_MAX`, as `u64` values do elsewhere.
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        zval.set_long(self as i64);
        zval
    }
}

impl Item for u8 {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        zval.set_long(i64::from(self));
        zval
    }

    /// Bytes come as a binary string, as `list<u8>` does.
    fn chunk(items: Vec<Self>) -> Zval {
        let mut zval = Zval::new();
        zval.set_binary(items);
        zval
    }
}

impl Item for bool {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        zval.set_bool(self);
        zval
    }
}

impl Item for f32 {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        zval.set_double(f64::from(self));
        zval
    }
}

impl Item for f64 {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        zval.set_double(self);
        zval
    }
}

impl Item for char {
    fn to_zval(self) -> Zval {
        String::from(self).to_zval()
    }
}

impl Item for String {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        let _ = zval.set_string(&self, false);
        zval
    }
}

/// What the consumer and the reading PHP side share.
struct Inner<T> {
    items: Vec<T>,
    /// Set by a read, cleared once the consumer took a chunk.
    wanted: bool,
    /// The component closed its end, or the stream failed.
    ended: bool,
    /// PHP dropped its end.
    closed: bool,
    consumer: Option<Waker>,
    reader: Option<Waker>,
}

struct Shared<T>(Arc<Mutex<Inner<T>>>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Shared<T> {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Inner {
            items: Vec::new(),
            wanted: false,
            ended: false,
            closed: false,
            consumer: None,
            reader: None,
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner<T>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Asks the consumer for the next chunk.
    fn want(&self) {
        let waker = {
            let mut inner = self.lock();
            inner.wanted = true;
            inner.consumer.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Ready once a chunk is there or the stream ended.
    fn arrived(&self, cx: &Context<'_>) -> Poll<()> {
        let mut inner = self.lock();
        if !inner.items.is_empty() || inner.ended {
            return Poll::Ready(());
        }
        inner.reader = Some(cx.waker().clone());
        Poll::Pending
    }

    fn close(&self) {
        let waker = {
            let mut inner = self.lock();
            inner.closed = true;
            inner.consumer.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// Takes items out of the component's stream while PHP reads.
struct Pump<T>(Shared<T>);

impl<T> Drop for Pump<T> {
    fn drop(&mut self) {
        let waker = {
            let mut inner = self.0.lock();
            inner.ended = true;
            inner.reader.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T: wasmtime::component::Lift + Send + 'static> StreamConsumer<HostState> for Pump<T> {
    type Item = T;

    fn poll_consume(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<'_, HostState>,
        mut source: Source<'_, T>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut inner = self.0.lock();
        if inner.closed {
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        if !inner.wanted {
            if finish {
                return Poll::Ready(Ok(StreamResult::Cancelled));
            }
            inner.consumer = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let mut chunk = Vec::with_capacity(CHUNK);
        source.read(store, &mut chunk)?;
        if chunk.is_empty() {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        inner.items.extend(chunk);
        inner.wanted = false;
        let reader = inner.reader.take();
        drop(inner);
        if let Some(waker) = reader {
            waker.wake();
        }
        Poll::Ready(Ok(StreamResult::Completed))
    }
}

macro_rules! payloads {
    ($($variant:ident($ty:ty) = $kind:pat),* $(,)?) => {
        /// A piped stream, by payload type.
        enum Pipe {
            $($variant(Shared<$ty>)),*
        }

        impl Pipe {
            fn supports(element: &Type) -> bool {
                matches!(element, $($kind)|*)
            }

            fn start(
                ctx: &mut StoreContextMut<'_, HostState>,
                stream: StreamAny,
                element: &Type,
            ) -> wasmtime::Result<Self> {
                match element {
                    $($kind => {
                        let shared = Shared::new();
                        stream
                            .try_into_stream_reader::<$ty>()?
                            .pipe(&mut *ctx, Pump(shared.clone()))?;
                        Ok(Self::$variant(shared))
                    })*
                    other => Err(wasmtime::Error::msg(format!(
                        "streams of {} are not supported yet",
                        crate::component::wit_type(other)
                    ))),
                }
            }

            /// The next chunk, if one arrived.
            fn take(&self) -> Option<Zval> {
                match self {
                    $(Self::$variant(shared) => {
                        let items = std::mem::take(&mut shared.lock().items);
                        (!items.is_empty()).then(|| <$ty as Item>::chunk(items))
                    })*
                }
            }

            fn ended(&self) -> bool {
                match self {
                    $(Self::$variant(shared) => shared.lock().ended),*
                }
            }

            fn want(&self) {
                match self {
                    $(Self::$variant(shared) => shared.want()),*
                }
            }

            fn waiter(&self) -> Waiter {
                match self {
                    $(Self::$variant(shared) => Waiter::$variant(shared.clone())),*
                }
            }

            fn close(&self) {
                match self {
                    $(Self::$variant(shared) => shared.close()),*
                }
            }
        }

        /// What the event loop polls until a chunk arrives; Send, unlike Pipe's owner.
        enum Waiter {
            $($variant(Shared<$ty>)),*
        }

        impl Waiter {
            fn arrived(&self, cx: &Context<'_>) -> Poll<()> {
                match self {
                    $(Self::$variant(shared) => shared.arrived(cx)),*
                }
            }
        }
    };
}

payloads! {
    U8(u8) = Type::U8,
    S8(i8) = Type::S8,
    U16(u16) = Type::U16,
    S16(i16) = Type::S16,
    U32(u32) = Type::U32,
    S32(i32) = Type::S32,
    U64(u64) = Type::U64,
    S64(i64) = Type::S64,
    F32(f32) = Type::Float32,
    F64(f64) = Type::Float64,
    Bool(bool) = Type::Bool,
    Char(char) = Type::Char,
    String(String) = Type::String,
}

enum State {
    /// Not read yet, so it can still be closed without the event loop.
    Unread(StreamAny, Type),
    Reading(Pipe),
    Ended,
}

/// A `stream<T>` a component returned. `read()` gives the next chunk:
/// a binary string for `stream<u8>`, a list of values otherwise, `null` at
/// the end. Iterating gives the chunks too.
#[php_class]
#[php(name = "Wasm\\Component\\Stream")]
#[php(flags = ClassFlags::Final)]
#[php(implements(ce = ce::iterator, stub = "\\Iterator"))]
pub struct Stream {
    store: SharedStore,
    state: RefCell<State>,
    current: RefCell<Option<Zval>>,
    position: std::cell::Cell<i64>,
    started: std::cell::Cell<bool>,
}

impl Stream {
    /// The PHP value of a stream the component handed over.
    pub fn lift(
        ctx: &mut StoreContextMut<'_, HostState>,
        stream: &StreamAny,
        element: Option<Type>,
    ) -> Result<Zval, ConvertError> {
        let Some(element) = element.clone().filter(Pipe::supports) else {
            let mut stream = stream.clone();
            let _ = stream.close(&mut *ctx);
            return Err(ConvertError::Runtime(format!(
                "streams of {} are not supported yet",
                element.map_or_else(
                    || "nothing".to_string(),
                    |ty| crate::component::wit_type(&ty)
                )
            )));
        };
        Self {
            store: store::of(&*ctx),
            state: RefCell::new(State::Unread(stream.clone(), element)),
            current: RefCell::new(None),
            position: std::cell::Cell::new(0),
            started: std::cell::Cell::new(false),
        }
        .into_zval(false)
        .map_err(|err| ConvertError::Value(err.to_string()))
    }

    fn next_chunk(&self) -> PhpResult<Option<Zval>> {
        if self.store.is_parked() {
            return Err(store::busy());
        }
        let mut state = self.state.borrow_mut();
        loop {
            match &*state {
                State::Ended => return Ok(None),
                State::Unread(stream, element) => {
                    let (stream, element) = (stream.clone(), element.clone());
                    let pipe = self
                        .store
                        .with(|mut ctx| Pipe::start(&mut ctx, stream, &element))
                        .map_err(runtime_error)?;
                    *state = State::Reading(pipe);
                }
                State::Reading(pipe) => {
                    if let Some(chunk) = pipe.take() {
                        return Ok(Some(chunk));
                    }
                    if pipe.ended() {
                        *state = State::Ended;
                        return Ok(None);
                    }
                    pipe.want();
                    let waiter = pipe.waiter();
                    let store = self.store.clone();
                    self.store
                        .with(|ctx| {
                            crate::suspend::drive_until_idle(
                                &store,
                                ctx.run_concurrent(async move |_| {
                                    std::future::poll_fn(|cx| waiter.arrived(cx)).await
                                }),
                            )
                        })
                        .map_err(runtime_error)?;
                }
            }
        }
    }
}

#[php_impl]
impl Stream {
    /// The next chunk, or `null` once the stream ended.
    ///
    /// @return string|list<mixed>|null
    pub fn read(&self) -> PhpResult<Zval> {
        Ok(self.next_chunk()?.unwrap_or_else(Zval::null))
    }

    /// @return string|list<mixed>|null
    pub fn current(&self) -> Zval {
        self.current
            .borrow()
            .as_ref()
            .map_or_else(Zval::null, Zval::shallow_clone)
    }

    pub fn key(&self) -> i64 {
        self.position.get()
    }

    pub fn next(&self) -> PhpResult<()> {
        self.position.set(self.position.get() + 1);
        *self.current.borrow_mut() = self.next_chunk()?;
        Ok(())
    }

    /// Starts reading; a stream cannot be read twice, so later calls do nothing.
    pub fn rewind(&self) -> PhpResult<()> {
        if !self.started.replace(true) {
            *self.current.borrow_mut() = self.next_chunk()?;
        }
        Ok(())
    }

    pub fn valid(&self) -> bool {
        self.current.borrow().is_some()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        match std::mem::replace(self.state.get_mut(), State::Ended) {
            State::Unread(stream, _) => self.store.close_stream(stream),
            State::Reading(pipe) => pipe.close(),
            State::Ended => {}
        }
    }
}
