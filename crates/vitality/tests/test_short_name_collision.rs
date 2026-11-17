//! The facade's short-name rule, asserted at the facade: the crash is easy to pin, but the hint a
//! two-crate author has to act on is the part worth an assert.

use bevy::prelude::*;
use vitality::{AttributesPlugin, init_attribute};

mod game_a {
    use vitality::prelude::*;
    vitality::attribute!(Health, f32);
    vitality::attribute!(Armor, f32);
}

mod game_b {
    use vitality::prelude::*;
    vitality::attribute!(Health, f32);
    vitality::attribute!(Mana, f32);
}

/// called, not passed to `add_plugins`: two values of type `fn(&mut App)` carry one plugin type id,
/// so bevy rejects the second before either attribute is registered
fn app_with(game: &[fn(&mut App)]) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    for register in game {
        register(&mut app);
    }
    app
}

#[test]
#[should_panic(expected = "already holds the short name: rename one of them")]
fn two_crates_sharing_a_short_name_are_told_to_rename_one() {
    let _shared = app_with(&[
        init_attribute::<game_a::Health>,
        init_attribute::<game_b::Health>,
    ]);
}
