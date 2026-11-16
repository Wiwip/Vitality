mod gameplay;

use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
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

    let registered = aura.clone();
    let global = with_vitality(&mut app, move |vitality| {
        vitality.add_global_effect(registered);
        vitality.global_actor()
    })
    .expect("PreStartup spawns the global actor before the first update");

    let later = spawn(&mut app, hero_def());
    ticks(&mut app, 2);

    let probe = {
        let registered = aura.clone();
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
