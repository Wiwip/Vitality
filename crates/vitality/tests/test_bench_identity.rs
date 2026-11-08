//! P-16: per-actor identity. `common::prepare_world` builds one `ActorBuilder` template, and
//! `SpawnActorCommand` (`crates/vitality/src/actors.rs:64-68`) replays the same `builder_actions` for
//! every spawn, so every actor a shape spawns carries the same value for every attribute — and a
//! harness guard that reads the wrong actor passes just as loudly. The first half of the test pins
//! that hole, the second half is what `benches/support/identity.rs` closes.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/identity.rs"]
mod identity;

use common::{HarnessWorld, WorldSpec, actor_entities, current_values, tick};
use identity::{assert_distinguishable, stagger_actors};

/// The shape the item names: `a=3/actors=4`, permanent modifiers so `current_value` sits above
/// `base_value` and a base write is something the tick can resolve.
fn tiny_spec() -> WorldSpec {
    WorldSpec {
        attributes_per_actor: 3,
        effects_per_actor: 2,
        modifiers_per_effect: 1,
        actors: 4,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    }
}

#[test]
fn actors_are_distinguishable_after_staggering() {
    let mut app = tiny_spec().build_app();
    let pool = app.world().resource::<HarnessWorld>().attrs();
    let actors = actor_entities(app.world_mut());
    assert_eq!(
        actors.len(),
        tiny_spec().actors,
        "one entity per spec.actors"
    );

    // Resolve first: an unsettled world is a second thing every actor shares by construction, and
    // comparing those values would say nothing about the generators.
    tick(&mut app);

    let first = current_values(app.world(), pool, actors[0]);
    assert!(
        first
            .iter()
            .enumerate()
            .any(|(index, value)| *value != pool[index].base(app.world(), actors[0]).unwrap()),
        "every settled value on {:?} still equals its own base_value ({first:?}), so this test cannot \
         tell a resolved value from a placeholder and the equality below proves nothing",
        actors[0]
    );
    assert!(
        first.windows(2).any(|pair| pair[0] != pair[1]),
        "the shape drew one value for every attribute ({first:?}), so a per-actor difference would \
         have nothing to show up in",
    );

    for &actor in actors.iter().skip(1) {
        assert_eq!(
            current_values(app.world(), pool, actor),
            first,
            "a built world is no longer value-identical: {actor:?} differs from {:?}. This half of \
             the test exists to pin the hole (`prepare_world` hands every spawn the same template), \
             so if the generators ever go per-actor, delete this loop rather than the stagger.",
            actors[0]
        );
    }

    let wrote = stagger_actors(app.world_mut(), pool, &actors);
    assert_eq!(
        wrote,
        actors.len() * pool.len(),
        "stagger_actors wrote {wrote} bases, not one per actor per attribute of {} actors",
        actors.len()
    );
    tick(&mut app);

    // The same `current_values` comparison that just reported four identical actors now reports
    // every pair different, which is what makes the loop above a measurement and not a tautology.
    assert_distinguishable(app.world(), pool, &actors);
}
