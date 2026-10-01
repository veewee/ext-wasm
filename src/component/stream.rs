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
use wasmtime::component::{
    Destination, FutureAny, FutureConsumer, FutureReader, Source, StreamAny, StreamConsumer,
    StreamProducer, StreamReader, StreamResult, Val, VecBuffer,
};

use crate::component::value::scalar;
use crate::error::runtime_error;
use crate::store::{self, Active, HostState, SharedStore, Unread, ValueKey};
use crate::suspend::{Callee, Request};
use crate::value::{ConvertError, debug_type};

/// The most items one read takes from the component.
const CHUNK: usize = 64 * 1024;

/// Converts the items of one chunk to PHP, and PHP values to items.
trait Item: Sized + Send + Sync + 'static {
    fn to_zval(self) -> Zval;

    fn from_val(val: Val) -> Option<Self>;

    /// Adds what one element a PHP iterable yields stands for.
    fn extend(value: &Zval, element: &Type, items: &mut Vec<Self>) -> Result<(), ConvertError> {
        let val = scalar(value, element)?;
        items.push(
            Self::from_val(val).ok_or_else(|| ConvertError::Runtime("unexpected value".into()))?,
        );
        Ok(())
    }

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
    ($($ty:ty = $val:ident),*) => {$(
        impl Item for $ty {
            fn to_zval(self) -> Zval {
                let mut zval = Zval::new();
                zval.set_long(self as i64);
                zval
            }

            fn from_val(val: Val) -> Option<Self> {
                match val {
                    Val::$val(n) => Some(n),
                    _ => None,
                }
            }
        }
    )*};
}

// u64 keeps its bits above PHP_INT_MAX, as u64 values do elsewhere.
long_items!(
    i8 = S8,
    i16 = S16,
    u16 = U16,
    i32 = S32,
    u32 = U32,
    i64 = S64,
    u64 = U64
);

impl Item for u8 {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        zval.set_long(i64::from(self));
        zval
    }

    fn from_val(val: Val) -> Option<Self> {
        match val {
            Val::U8(n) => Some(n),
            _ => None,
        }
    }

    /// A string adds its bytes, so a `stream<u8>` can be fed string chunks.
    fn extend(value: &Zval, element: &Type, items: &mut Vec<Self>) -> Result<(), ConvertError> {
        if let Some(bytes) = value.zend_str().filter(|_| value.is_string()) {
            items.extend_from_slice(bytes.as_bytes());
            return Ok(());
        }
        items.push(Self::from_val(scalar(value, element)?).unwrap_or_default());
        Ok(())
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

    fn from_val(val: Val) -> Option<Self> {
        match val {
            Val::Bool(b) => Some(b),
            _ => None,
        }
    }
}

impl Item for f32 {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        zval.set_double(f64::from(self));
        zval
    }

    fn from_val(val: Val) -> Option<Self> {
        match val {
            Val::Float32(n) => Some(n),
            _ => None,
        }
    }
}

impl Item for f64 {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        zval.set_double(self);
        zval
    }

    fn from_val(val: Val) -> Option<Self> {
        match val {
            Val::Float64(n) => Some(n),
            _ => None,
        }
    }
}

impl Item for char {
    fn to_zval(self) -> Zval {
        String::from(self).to_zval()
    }

    fn from_val(val: Val) -> Option<Self> {
        match val {
            Val::Char(c) => Some(c),
            _ => None,
        }
    }
}

impl Item for String {
    fn to_zval(self) -> Zval {
        let mut zval = Zval::new();
        let _ = zval.set_string(&self, false);
        zval
    }

    fn from_val(val: Val) -> Option<Self> {
        match val {
            Val::String(s) => Some(s),
            _ => None,
        }
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

        /// A piped future, by payload type.
        enum Awaiting {
            $($variant(Arc<Mutex<Settled<$ty>>>)),*
        }

        impl Awaiting {
            fn start(
                ctx: &mut StoreContextMut<'_, HostState>,
                future: FutureAny,
                element: &Type,
            ) -> wasmtime::Result<Self> {
                match element {
                    $($kind => {
                        let settled = Arc::new(Mutex::new(Settled { value: None, ended: false, reader: None }));
                        future
                            .try_into_future_reader::<$ty>()?
                            .pipe(&mut *ctx, Take(settled.clone()))?;
                        Ok(Self::$variant(settled))
                    })*
                    other => Err(wasmtime::Error::msg(format!(
                        "futures of {} are not supported yet",
                        crate::component::wit_type(other)
                    ))),
                }
            }

            /// `Some` once settled: the value, or `None` when the future closed without one.
            fn settled(&self) -> Option<Option<Zval>> {
                match self {
                    $(Self::$variant(settled) => {
                        let mut settled = settled.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        match settled.value.take() {
                            Some(value) => Some(Some(value.to_zval())),
                            None => settled.ended.then_some(None),
                        }
                    })*
                }
            }

            fn waiter(&self) -> FutureWaiter {
                match self {
                    $(Self::$variant(settled) => FutureWaiter::$variant(settled.clone())),*
                }
            }
        }

        enum FutureWaiter {
            $($variant(Arc<Mutex<Settled<$ty>>>)),*
        }

        impl FutureWaiter {
            fn arrived(&self, cx: &Context<'_>) -> Poll<()> {
                match self {
                    $(Self::$variant(settled) => {
                        let mut settled = settled.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        if settled.value.is_some() || settled.ended {
                            return Poll::Ready(());
                        }
                        settled.reader = Some(cx.waker().clone());
                        Poll::Pending
                    })*
                }
            }
        }

        fn supports_future(element: &Type) -> bool {
            matches!(element, $($kind)|*)
        }

        /// A component future that is ready at once with a PHP value.
        pub fn ready_future(
            ctx: &mut StoreContextMut<'_, HostState>,
            value: &Zval,
            element: Option<Type>,
        ) -> Result<Val, ConvertError> {
            let runtime = |err: wasmtime::Error| ConvertError::Runtime(format!("{err:#}"));
            match &element {
                $(Some(element @ $kind) => {
                    let value = <$ty as Item>::from_val(scalar(value, element)?)
                        .ok_or_else(|| ConvertError::Runtime("unexpected value".into()))?;
                    let reader = FutureReader::new(&mut *ctx, std::future::ready(Ok::<_, wasmtime::Error>(value)))
                        .map_err(runtime)?;
                    Ok(Val::Future(reader.try_into_future_any(&mut *ctx).map_err(runtime)?))
                })*
                _ => Err(ConvertError::Runtime(format!(
                    "futures of {} are not supported yet",
                    element.map_or_else(|| "nothing".to_string(), |ty| crate::component::wit_type(&ty))
                ))),
            }
        }

        /// A component stream fed by a PHP iterable.
        pub fn feed(
            ctx: &mut StoreContextMut<'_, HostState>,
            value: &Zval,
            element: Option<Type>,
        ) -> Result<Val, ConvertError> {
            match &element {
                $(Some(element @ $kind) => feed_with::<$ty>(ctx, value, element),)*
                _ => Err(ConvertError::Runtime(format!(
                    "streams of {} are not supported yet",
                    element.map_or_else(|| "nothing".to_string(), |ty| crate::component::wit_type(&ty))
                ))),
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

/// Whether a stream or future of `element` can cross to PHP: scalar
/// payloads only, since wasmtime's typed stream API needs the Rust type.
pub fn supports(element: Option<&Type>) -> bool {
    element.is_some_and(Pipe::supports)
}

/// What a feeding producer and the PHP side advancing its iterator share.
struct FeedInner<T> {
    items: Vec<T>,
    started: bool,
    done: bool,
    failed: bool,
    /// A request to advance the iterator is out.
    asked: bool,
    waker: Option<Waker>,
}

/// Feeds a component stream from a PHP iterable. It holds no PHP value, so
/// wasmtime may drop it anywhere: the iterator stays in the store's values,
/// and each chunk is produced by the poll loop on the PHP stack.
struct Feed<T> {
    shared: Arc<Mutex<FeedInner<T>>>,
    iterator: Option<ValueKey>,
    element: Type,
}

fn feed_with<T>(
    ctx: &mut StoreContextMut<'_, HostState>,
    value: &Zval,
    element: &Type,
) -> Result<Val, ConvertError>
where
    T: Item + wasmtime::component::Lower + wasmtime::component::Lift,
{
    let expected = || {
        ConvertError::Type(format!(
            "expected iterable for stream<{}>, got {}",
            crate::component::wit_type(element),
            debug_type(value)
        ))
    };
    let mut items = Vec::new();
    let iterator = if let Some(array) = value.array() {
        for item in array.values() {
            T::extend(item, element, &mut items)?;
        }
        None
    } else {
        let object = value.object().ok_or_else(expected)?;
        if !object.instance_of(ce::traversable()) {
            return Err(expected());
        }
        let mut iterator = value.shallow_clone();
        while let Some(object) = iterator
            .object()
            .filter(|object| object.instance_of(ce::aggregate()))
        {
            iterator = object
                .try_call_method("getIterator", vec![])
                .map_err(|err| ConvertError::Error(err.to_string()))?;
        }
        if !iterator
            .object()
            .is_some_and(|object| object.instance_of(ce::iterator()))
        {
            return Err(expected());
        }
        Some(ctx.data_mut().values.insert_ref(iterator))
    };
    let feed = Feed {
        shared: Arc::new(Mutex::new(FeedInner {
            items,
            started: false,
            done: iterator.is_none(),
            failed: false,
            asked: false,
            waker: None,
        })),
        iterator,
        element: element.clone(),
    };
    let reader = StreamReader::new(&mut *ctx, feed)
        .map_err(|err| ConvertError::Runtime(format!("{err:#}")))?;
    Ok(Val::Stream(reader.try_into_stream_any(&mut *ctx).map_err(
        |err| ConvertError::Runtime(format!("{err:#}")),
    )?))
}

impl<T> StreamProducer<HostState> for Feed<T>
where
    T: Item + wasmtime::component::Lower + wasmtime::component::Lift,
{
    type Item = T;
    type Buffer = VecBuffer<T>;

    fn poll_produce<'a>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<'a, HostState>,
        mut destination: Destination<'a, T, VecBuffer<T>>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let this = self.get_mut();
        let mut inner = this
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.failed {
            // Its exception stays pending, and the PHP entry point rethrows it.
            return Poll::Ready(Err(wasmtime::Error::msg(
                "the PHP iterable feeding the stream failed",
            )));
        }
        if !inner.items.is_empty() {
            destination.set_buffer(VecBuffer::from(std::mem::take(&mut inner.items)));
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        if inner.done {
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        if finish {
            return Poll::Ready(Ok(StreamResult::Cancelled));
        }
        inner.waker = Some(cx.waker().clone());
        if !inner.asked {
            inner.asked = true;
            drop(inner);
            let Some(key) = &this.iterator else {
                return Poll::Pending;
            };
            let iterator = store.data().values.get(key.key()).shallow_clone();
            let shared = this.shared.clone();
            let element = this.element.clone();
            let handle = store::of(&store);
            let id = handle.next_request_id();
            handle.put_request(Request {
                id,
                access: Active::Unavailable,
                callee: Callee::Feed(Box::new(move || advance(&iterator, &shared, &element))),
                args: Vec::new(),
                suspending: false,
            });
        }
        Poll::Pending
    }
}

/// Moves the PHP iterator one element on and leaves its items for the producer.
fn advance<T: Item>(iterator: &Zval, shared: &Arc<Mutex<FeedInner<T>>>, element: &Type) {
    let lock = || {
        shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    };
    let started = std::mem::replace(&mut lock().started, true);
    let mut items = Vec::new();
    let outcome: Result<bool, Option<ConvertError>> = (|| {
        let object = iterator.object().ok_or(None)?;
        object
            .try_call_method(if started { "next" } else { "rewind" }, vec![])
            .map_err(|_| None)?;
        let valid = object.try_call_method("valid", vec![]).map_err(|_| None)?;
        if !valid.bool().unwrap_or(false) {
            return Ok(false);
        }
        let current = object
            .try_call_method("current", vec![])
            .map_err(|_| None)?;
        T::extend(&current, element, &mut items).map_err(Some)?;
        Ok(true)
    })();
    let waker = {
        let mut inner = lock();
        inner.asked = false;
        match outcome {
            Ok(true) => inner.items.extend(items),
            Ok(false) => inner.done = true,
            Err(error) => {
                if let Some(error) = error {
                    // An iterator method that threw left its exception pending already.
                    ext_php_rs::exception::PhpException::from(error).throw();
                }
                inner.failed = true;
            }
        }
        inner.waker.take()
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// The value of a component future, once it arrived.
struct Settled<T> {
    value: Option<T>,
    /// The future closed, with or without a value.
    ended: bool,
    reader: Option<Waker>,
}

/// Takes the value of a component future.
struct Take<T>(Arc<Mutex<Settled<T>>>);

impl<T> Drop for Take<T> {
    fn drop(&mut self) {
        let waker = {
            let mut settled = self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settled.ended = true;
            settled.reader.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T: wasmtime::component::Lift + Send + 'static> FutureConsumer<HostState> for Take<T> {
    type Item = T;

    fn poll_consume(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<'_, HostState>,
        mut source: Source<'_, T>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<()>> {
        let mut value = None;
        source.read(store, &mut value)?;
        let waker = {
            let mut settled = self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settled.value = value;
            settled.reader.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        Poll::Ready(Ok(()))
    }
}

enum Pending {
    /// Not awaited yet, so it can still be closed without the event loop.
    Unread(FutureAny, Type),
    Awaiting(Awaiting),
    Settled(Zval),
}

/// A `future<T>` a component returned. `await()` runs the component until
/// its value is there and returns it, the same value on every call.
#[php_class]
#[php(name = "Wasm\\Component\\Future")]
#[php(flags = ClassFlags::Final)]
pub struct Future {
    store: SharedStore,
    state: RefCell<Pending>,
}

impl Future {
    /// The PHP value of a future the component handed over.
    pub fn lift(
        ctx: &mut StoreContextMut<'_, HostState>,
        future: &FutureAny,
        element: Option<Type>,
    ) -> Result<Zval, ConvertError> {
        let Some(element) = element.clone().filter(supports_future) else {
            let mut future = future.clone();
            let _ = future.close(&mut *ctx);
            return Err(ConvertError::Runtime(format!(
                "futures of {} are not supported yet",
                element.map_or_else(
                    || "nothing".to_string(),
                    |ty| crate::component::wit_type(&ty)
                )
            )));
        };
        Self {
            store: store::of(&*ctx),
            state: RefCell::new(Pending::Unread(future.clone(), element)),
        }
        .into_zval(false)
        .map_err(|err| ConvertError::Value(err.to_string()))
    }
}

#[php_impl]
impl Future {
    /// The value of the future, once the component wrote it.
    #[php(name = "await")]
    pub fn await_(&self) -> PhpResult<Zval> {
        if self.store.is_parked() {
            return Err(store::busy());
        }
        let mut state = self.state.borrow_mut();
        loop {
            match &*state {
                Pending::Settled(value) => return Ok(value.shallow_clone()),
                Pending::Unread(future, element) => {
                    let (future, element) = (future.clone(), element.clone());
                    let awaiting = self
                        .store
                        .with(|mut ctx| Awaiting::start(&mut ctx, future, &element))
                        .map_err(runtime_error)?;
                    *state = Pending::Awaiting(awaiting);
                }
                Pending::Awaiting(awaiting) => match awaiting.settled() {
                    Some(Some(value)) => *state = Pending::Settled(value),
                    Some(None) => {
                        return Err(runtime_error(wasmtime::Error::msg(
                            "the component closed the future without a value",
                        )));
                    }
                    None => {
                        let waiter = awaiting.waiter();
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
                },
            }
        }
    }
}

impl Drop for Future {
    fn drop(&mut self) {
        if let Pending::Unread(future, _) =
            std::mem::replace(self.state.get_mut(), Pending::Settled(Zval::null()))
        {
            self.store.close_unread(Unread::Future(future));
        }
    }
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
            State::Unread(stream, _) => self.store.close_unread(Unread::Stream(stream)),
            State::Reading(pipe) => pipe.close(),
            State::Ended => {}
        }
    }
}
