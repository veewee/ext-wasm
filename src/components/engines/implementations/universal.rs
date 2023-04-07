use std::collections::HashMap;

use ext_php_rs::builders::ClassBuilder;
use ext_php_rs::builders::FunctionBuilder;
use ext_php_rs::class::ClassMetadata;
use ext_php_rs::class::ConstructorMeta;
use ext_php_rs::class::ConstructorResult;
use ext_php_rs::class::RegisteredClass;
use ext_php_rs::flags::ClassFlags;
use ext_php_rs::props::Property;
use ext_php_rs::zend::ExecuteData;

use crate::components::engines::interfaces::ENGINE_INTERFACE_CE;

const UNIVERSAL_IMPLMENTATION_CLASS_NAME: &str = "Wasm\\Engine\\Universal";

static UNIVERSAL_IMPLMENTATION_METADATA: ClassMetadata<Universal> = ClassMetadata::new();

pub fn build() {
    let builder = ClassBuilder::new(UNIVERSAL_IMPLMENTATION_CLASS_NAME)
        .flags(ClassFlags::Final | ClassFlags::NoDynamicProperties)
        .implements(unsafe { ENGINE_INTERFACE_CE.unwrap() })
        .object_override::<Universal>();

    let class = builder
        .build()
        .unwrap_or_else(|_| panic!("Unable to build class `{}`", UNIVERSAL_IMPLMENTATION_CLASS_NAME));

    UNIVERSAL_IMPLMENTATION_METADATA.set_ce(class);
}

pub struct Universal {
    inner: wasmer::Store,
}

impl Universal {
    pub fn construct(ex: &mut ExecuteData) -> ConstructorResult<Self> {
        if ex.parser().parse().is_err() {
            return ConstructorResult::ArgError;
        }

        Self { inner: wasmer::Store::default() }.into()
    }
}

impl RegisteredClass for Universal {
    const CLASS_NAME: &'static str = UNIVERSAL_IMPLMENTATION_CLASS_NAME;

    const CONSTRUCTOR: Option<ConstructorMeta<Self>> = Some(ConstructorMeta {
        constructor: Self::construct,
        build_fn: |func: FunctionBuilder| -> FunctionBuilder { func },
    });

    fn get_metadata() -> &'static ClassMetadata<Self> {
        &UNIVERSAL_IMPLMENTATION_METADATA
    }

    fn get_properties<'a>() -> HashMap<&'static str, Property<'a, Self>> {
        HashMap::new()
    }
}

implement_class!(Universal, UNIVERSAL_IMPLMENTATION_CLASS_NAME);
