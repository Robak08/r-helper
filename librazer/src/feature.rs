use const_format::{map_ascii_case, Case};

pub trait Feature {
    fn name(&self) -> &'static str;
}

macro_rules! feature_list {
    ($($type:ident,)*) => {
        $(
            #[derive(Default)]
            pub struct $type {}

            impl Feature for $type {
                fn name(&self) -> &'static str {
                    map_ascii_case!(Case::Kebab, stringify!($type))
                }
            }
        )*

        pub const ALL_FEATURES: &[&'static str] = &[
            $(map_ascii_case!(Case::Kebab, stringify!($type)),)*
        ];
    }
}

feature_list![BatteryCare, LidLogo, LightsAlwaysOn, KbdBacklight, Fan, Perf,];
