//! The facade's short-name rule, asserted at the facade. The crate-page doctest that shows the
//! collision is a bare `should_panic`, so it can pin the crash but not the hint, and the hint is the
//! part a two-crate author has to be able to act on.

use bevy::prelude::*;
use vitality::{AttributesPlugin, init_attribute};

/// The two crates of the crate-page example: one name each that is fine, and one they share.
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

/// The cap pin's shape: [`AttributesPlugin`] first, then each attribute through the facade's
/// registration path. Called, not passed to `add_plugins`: two values of type `fn(&mut App)` carry
/// one plugin type id, so bevy rejects the second before either attribute is registered.
fn app_with(game: &[fn(&mut App)]) -> App {
    // the collision is a registration-time insert, so the resolver mode plays no part in it
    let mut app = App::new();
    app.insert_resource(vitality::ResolverMode::Dag)
        .add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    for register in game {
        register(&mut app);
    }
    app
}

/// The shared short name does not build — and the hint names the short name and the fix instead of
/// a duplicate registration.
#[test]
#[should_panic(expected = "already holds the short name: rename one of them")]
fn two_crates_sharing_a_short_name_are_told_to_rename_one() {
    let _shared = app_with(&[
        init_attribute::<game_a::Health>,
        init_attribute::<game_b::Health>,
    ]);
}
