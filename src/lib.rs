#![cfg_attr(windows, feature(abi_vectorcall))]

mod callback;
mod component;
mod engine;
mod error;
mod exports;
mod func;
mod functions;
mod global;
mod imports;
mod instance;
mod memory;
mod module;
mod store;
mod suspend;
mod table;
mod tag;
mod throw;
mod value;
mod wasi;

use ext_php_rs::flags::IniEntryPermission;
use ext_php_rs::prelude::*;
use ext_php_rs::zend::{IniEntryDef, IniEntryDefs, ModuleEntry};
use ext_php_rs::{info_table_end, info_table_row, info_table_start};

/// Used by `phpinfo()` and `php -i`.
pub extern "C" fn php_module_info(_module: *mut ModuleEntry) {
    info_table_start!();
    info_table_row!("wasm support", "enabled");
    info_table_row!("wasm runtime", "wasmtime 49.0.1");
    info_table_end!();
}

static INI_ENTRIES: IniEntryDefs<3> = IniEntryDefs::new([
    // System only: the engine is created once per process, so a later
    // ini_set() could not change anything.
    IniEntryDef::new(c"wasm.cache", c"1", IniEntryPermission::System),
    IniEntryDef::new(c"wasm.cache_dir", c"", IniEntryPermission::System),
    IniEntryDef::end(),
]);

fn startup(_type: i32, module_number: i32) -> i32 {
    IniEntryDef::register(INI_ENTRIES.as_slice(), module_number);
    #[cfg(windows)]
    pin_module();
    0
}

/// Keeps this DLL loaded until the process exits.
///
/// PHP unloads extensions at shutdown, but wasmtime's compile and cache threads
/// may still be running code from this DLL then, and they crashed with an
/// access violation once it was gone. The crash only showed on Windows, and
/// not at all with ZEND_DONT_UNLOAD_MODULES set.
#[cfg(windows)]
fn pin_module() {
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    };

    let mut module = std::ptr::null_mut();
    // SAFETY: the address of a function in this DLL identifies it, and the
    // handle is only written, never used.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            pin_module as *const () as *const u16,
            &mut module,
        );
    }
}

extern "C" fn request_startup(_type: i32, _module_number: i32) -> i32 {
    error::adopt_exception_behaviour();
    0
}

#[php_module]
#[php(startup = startup)]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    functions::register(module)
        .info_function(php_module_info)
        .request_startup_function(request_startup)
        .class::<error::WasmException>()
        .class::<error::CompileError>()
        .class::<error::LinkError>()
        .class::<error::RuntimeError>()
        .class::<throw::WasmThrow>()
        .class::<component::error::ComponentError>()
        .class::<module::Module>()
        .class::<component::Component>()
        .class::<component::func::Func>()
        .class::<component::exports::ExportsIterator>()
        .class::<component::exports::Exports>()
        .class::<component::instance::Instance>()
        .class::<component::resource::ResourceClass>()
        .class::<component::resource::Resource>()
        .class::<component::types::ValueType>()
        .class::<component::types::FunctionType>()
        .class::<component::http_handler::Request>()
        .class::<component::http_handler::Response>()
        .class::<component::value::Variant>()
        .class::<component::value::ResultValue>()
        .class::<store::StoreObject>()
        .class::<suspend::Suspending>()
        .class::<wasi::Wasi>()
        .class::<func::Func>()
        .class::<global::GlobalVar>()
        .class::<memory::Memory>()
        .class::<table::Table>()
        .class::<tag::Tag>()
        .class::<exports::Exports>()
        .class::<instance::Instance>()
}
