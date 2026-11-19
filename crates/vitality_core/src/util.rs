use bevy::prelude::ShortName;

pub fn pretty_type_name<T>() -> String {
    format!("{}", ShortName::of::<T>())
}
