mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::effect::GlobalEffectPlugin;
use vitality::prelude::*;

#[test]
fn a_global_effect_lands_on_actors_spawned_after_it_is_added() {
    let mut app = app();
    let earlier = spawn(&mut app, hero_def());
    let aura = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectApplied>(&mut app);

    let registered = aura;
    let global = with_vitality(&mut app, move |vitality| {
        vitality.add_global_effect(registered);
        vitality.global_actor()
    })
    .expect("PreStartup spawns the global actor before the first update");

    let later = spawn(&mut app, hero_def());
    ticks(&mut app, 2);

    let probe = {
        let registered = aura;
        with_effects(&mut app, move |effects| {
            (
                effects.has(later, &registered),
                effects.has(earlier, &registered),
            )
        })
    };
    assert!(
        probe.0,
        "an actor spawned after add_global_effect must carry it"
    );
    assert_eq!(current::<Power>(&app, later), 15.0);
    assert!(
        !probe.1,
        "registering a global effect never replays it onto the actors that already exist"
    );
    assert_eq!(current::<Power>(&app, earlier), 10.0);

    let applied = recorded::<EffectApplied>(&app);
    assert_eq!(
        applied.len(),
        1,
        "only the actor spawned later is applied to"
    );
    assert_eq!(applied[0].entity, later);
    assert_eq!(
        applied[0].source, global,
        "the global effect actor is the source"
    );

    assert_eq!(
        with_effects(&mut app, move |effects| effects.on(global).len()),
        0,
        "the global actor's own Add<ManageAttributes> fired in PreStartup, while the registry \
         was still empty, so it gets nothing"
    );
}

#[derive(Component, Copy, Clone)]
struct Prop;

#[test]
fn a_global_effect_reaches_an_entity_carrying_a_chosen_marker_tag() {
    let mut app = app();
    app.add_plugins(GlobalEffectPlugin::<Prop>::new());
    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    with_vitality(&mut app, move |vitality| {
        vitality.add_global_effect(blessing);
    });

    let post = app.world_mut().spawn((Prop, Health::new(100.0))).id();
    ticks(&mut app, 2);

    assert_eq!(
        with_effects(&mut app, move |effects| effects.on(post)).len(),
        1,
        "the marker tag is what pulls the global effect onto the entity"
    );
    assert_eq!(
        current::<Health>(&app, post),
        110.0,
        "the global bonus is live on the marker carrier"
    );
}
