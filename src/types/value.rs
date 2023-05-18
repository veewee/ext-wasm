use ext_php_rs::ZvalConvert;

#[derive(Clone, Copy, ZvalConvert)]
pub enum Value {
    /// A 32-bit integer.
    ///
    /// In Wasm integers are sign-agnostic, i.e. this can either be signed or unsigned.
    I32(i32),

    /// A 64-bit integer.
    ///
    /// In Wasm integers are sign-agnostic, i.e. this can either be signed or unsigned.
    I64(i64),

    /// A 32-bit float.
    F32(f32),

    /// A 64-bit float.
    F64(f64),

    /*
    TODO
    /// An `externref` value which can hold opaque data to the wasm instance itself.
    ExternRef(Option<ExternRef>),

    /// A first-class reference to a WebAssembly function.
    FuncRef(Option<Function>),

    /// A 128-bit number
    V128(u128),
     */
}

macro_rules! accessors {
    ($bind:ident $(($variant:ident($ty:ty) $get:ident $unwrap:ident $cvt:expr))*) => ($(
        /// Attempt to access the underlying value of this `Value`, returning
        /// `None` if it is not the correct type.
        pub fn $get(&self) -> Option<$ty> {
            if let Self::$variant($bind) = self {
                Some($cvt)
            } else {
                None
            }
        }

        /// Returns the underlying value of this `Value`, panicking if it's the
        /// wrong type.
        ///
        /// # Panics
        ///
        /// Panics if `self` is not of the right type.
        pub fn $unwrap(&self) -> $ty {
            self.$get().expect(concat!("expected ", stringify!($ty)))
        }
    )*)
}

impl Value {
    accessors! {
        e
        (I32(i32) i32 unwrap_i32 *e)
        (I64(i64) i64 unwrap_i64 *e)
        (F32(f32) f32 unwrap_f32 *e)
        (F64(f64) f64 unwrap_f64 *e)
        /* TODO
        (ExternRef(&Option<ExternRef>) externref unwrap_externref e)
        (FuncRef(&Option<Function>) funcref unwrap_funcref e)
        (V128(u128) v128 unwrap_v128 *e)
         */
    }
}


impl From<&wasmer::Value> for Value {
    fn from(value: &wasmer::Value) -> Self {
        match value {
            wasmer::Value::I32(_) => Self::I32(value.unwrap_i32()),
            wasmer::Value::I64(_) => Self::I64(value.unwrap_i64()),
            wasmer::Value::F32(_) => Self::F32(value.unwrap_f32()),
            wasmer::Value::F64(_) => Self::F64(value.unwrap_f64()),
            _ => Self::I32(0), // TODO -> can go away if everything is set!
            /* TODO
            wasmer::Value::V128 => Self::V128,
            wasmer::Value::ExternRef => Self::ExternRef,
            wasmer::Value::FuncRef => Self::FuncRef,
            */
        }
    }
}

impl Into<wasmer::Value> for Value {
    fn into(self) -> wasmer::Value {
        match self {
            Self::I32(_) => wasmer::Value::I32(self.unwrap_i32()),
            Self::I64(_) => wasmer::Value::I64(self.unwrap_i64()),
            Self::F32(_) => wasmer::Value::F32(self.unwrap_f32()),
            Self::F64(_) => wasmer::Value::F64(self.unwrap_f64()),
            /* TODO
            //Self::V128 => wasmer::Value::V128,
            //Self::ExternRef => wasmer::Value::ExternRef,
            //Self::FuncRef => wasmer::Value::FuncRef,
            */
        }
    }
}
