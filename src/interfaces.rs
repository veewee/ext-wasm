use ext_php_rs::builders::ClassBuilder;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::zend::ClassEntry;

pub static mut ENGINE_INTERFACE_CE: Option<&'static ClassEntry> = None;

pub fn build() {
    let engine_interface_ce = ClassBuilder::new("Wasm\\Store\\Store")
        .flags(ClassFlags::Interface)
        .build()
        .unwrap();

    unsafe {
        ENGINE_INTERFACE_CE.replace(engine_interface_ce);
    }
}
