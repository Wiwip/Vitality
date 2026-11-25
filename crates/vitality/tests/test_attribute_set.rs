mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use vitality::actors::ActorBuilder;
use vitality::attribute_set;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

attribute_set! { pub struct Vitals { Vigour: u32, Focus: u32 } }
attribute_set! { pub struct Reserves { Ward: u32, Grit: u32 } }

#[test]
fn a_named_set_registers_its_members_and_is_a_bundle_you_can_build() {
    let mut app = app();
    app.add_plugins(VitalityPlugin::new().set::<Vitals>());
    app.add_plugins(init_attribute_set::<Reserves>);

    let actor = spawn(
        &mut app,
        ActorBuilder::new()
            .name("Warder")
            .insert(Vitals::new(10, 20))
            .clamp::<Vigour>(0, Focus::src()),
    );
    let carrier = app
        .world_mut()
        .spawn(Reserves {
            Ward: Ward::new(3),
            ..default()
        })
        .id();
    ticks(&mut app, 1);

    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Vigour>(20, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    let boon = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Ward>(5, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, actor, actor, &blessing);
    apply(&mut app, carrier, carrier, &boon);
    ticks(&mut app, 1);

    assert_eq!(
        current::<Vigour>(&app, actor),
        20,
        "the member folded 10 + 20 and the clamp stopped it at the sibling member's limit"
    );
    assert_eq!(
        current::<Focus>(&app, actor),
        20,
        "the member no effect touches keeps the value the bundle was built with"
    );
    assert_eq!(
        current::<Ward>(&app, carrier),
        8,
        "the set registered through init_attribute_set folds too"
    );
    assert_eq!(
        current::<Grit>(&app, carrier),
        0,
        "a member left out of the struct literal starts zeroed"
    );
}
