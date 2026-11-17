//! L-2 guard, library half: `ActorRead::get` — the call `Attribute::at()` resolves to — hands back
//! the attribute it is asked for, for every attribute of the pool, on every actor of the world.

#[path = "../benches/support/common.rs"]
mod common;

use bevy::prelude::*;
use common::{
    ATTRIBUTE_POOL, HarnessWorld, WorldSpec, actor_entities, build_app_at, current_values, tick,
};
use std::any::type_name;
use std::collections::HashSet;
use vitality::ResolverMode;
use vitality::attributes::Attribute;
use vitality::effect::EffectsRef;
use vitality_core::view::ActorRead;

/// One attribute, read through the trait call so it cannot become `EntityRefExcept`'s inherent
/// `get`, which is not what the library does.
fn read<T: Attribute<Property = f32>>(view: &EffectsRef<'_, '_>) -> f64 {
    ActorRead::get::<T>(view)
        .unwrap_or_else(|| panic!("{} unreadable through the view", type_name::<T>()))
        .current_value() as f64
}

/// One read per attribute of the pool, in pool order, so the guard covers every attribute an actor
/// carries instead of only slot 0.
macro_rules! read_pool {
    ($view:expr, $( $name:ident ),+ $(,)?) => {
        vec![$( read::<common::$name>($view) ),+]
    };
}

#[test]
fn the_view_hands_back_the_attribute_it_is_asked_for() {
    let spec = WorldSpec {
        attributes_per_actor: common::POOL_LEN,
        actors: 8,
        ..WorldSpec::default()
    };
    let mut app = build_app_at(&spec, ATTRIBUTE_POOL, ResolverMode::Dag);
    // build_app_at only runs Startup, so nothing has resolved and every current_value is still a base.
    tick(&mut app);
    let pool = app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        pool.len(),
        common::POOL_LEN,
        "the reads below cover {} of the {} attributes this world carries",
        common::POOL_LEN,
        pool.len()
    );

    let mut views = app.world_mut().query::<EffectsRef<'static, 'static>>();
    for actor in actor_entities(app.world_mut()) {
        let expected = current_values(app.world(), pool, actor);
        let distinct: HashSet<u64> = expected.iter().map(|value| value.to_bits()).collect();
        assert_eq!(
            distinct.len(),
            expected.len(),
            "two pool attributes share a current value on {actor}, so a swapped hand-back could \
             compare equal"
        );
        let view = views
            .get(app.world(), actor)
            .unwrap_or_else(|err| panic!("no attribute view on {actor}: {err}"));
        assert_eq!(
            read_pool!(
                &view, Attr0, Attr1, Attr2, Attr3, Attr4, Attr5, Attr6, Attr7, Attr8, Attr9,
                Attr10, Attr11, Attr12, Attr13, Attr14, Attr15, Attr16, Attr17, Attr18, Attr19,
                Attr20, Attr21, Attr22, Attr23
            ),
            expected,
            "one read per pool attribute on {actor} of shape {}",
            spec.shape_id()
        );
    }
}
