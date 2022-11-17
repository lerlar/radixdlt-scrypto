#[macro_export]
macro_rules! construct_address {
    (EntityType::Resource, $($bytes:expr),*) => {
        ::radix_engine_lib::resource::ResourceAddress::Normal([$($bytes),*])
    };
    (EntityType::Package, $($bytes:expr),*) => {
        ::radix_engine_lib::component::PackageAddress::Normal([$($bytes),*])
    };
    (EntityType::NormalComponent, $($bytes:expr),*) => {
        ::radix_engine_lib::model::ComponentAddress::Normal([$($bytes),*])
    };
    (EntityType::AccountComponent, $($bytes:expr),*) => {
        ::radix_engine_lib::model::ComponentAddress::Account([$($bytes),*])
    };
    (EntityType::EpochManager, $($bytes:expr),*) => {
        ::radix_engine_lib::model::SystemAddress::EpochManager([$($bytes),*])
    };
}

#[macro_export]
macro_rules! address {
    (EntityType::$entity_type:tt, $last_byte:literal) => {
        ::radix_engine_lib::construct_address!(
            EntityType::$entity_type,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            $last_byte
        )
    };
    (EntityType::$entity_type:tt, [$repeat_byte:literal; 26]) => {
        ::radix_engine_lib::construct_address!(
            EntityType::$entity_type,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte,
            $repeat_byte
        )
    };
    (EntityType::$entity_type:tt, $($bytes:literal),*) => {
        ::radix_engine_lib::construct_address!(EntityType::$entity_type, $($bytes),*)
    };
}
