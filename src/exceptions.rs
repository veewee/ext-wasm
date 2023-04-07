use ext_php_rs::builders::ClassBuilder;
use ext_php_rs::zend::ce;
use ext_php_rs::zend::ClassEntry;

pub static mut RUNTIME_EXCEPTION_CE: Option<&'static ClassEntry> = None;

pub fn build() {
    let spl_runtime_exception_ce = ClassEntry::try_find("RuntimeException").unwrap_or_else(|| {
        ClassBuilder::new("RuntimeException")
            .extends(ce::exception())
            .build()
            .unwrap()
    });

    let runtime_exception_ce = ClassBuilder::new("Wasm\\Exception\\RuntimeException")
        .extends(spl_runtime_exception_ce)
        .build()
        .unwrap();

    unsafe {
        RUNTIME_EXCEPTION_CE.replace(runtime_exception_ce);
    }
}