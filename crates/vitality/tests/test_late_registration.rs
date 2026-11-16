//! The wiring contract's first rule from the bad side: an attribute registered in `Plugin::finish`
//! is never registered at all, and everything downstream of it stays silent.

mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use vitality::CurrentValueChanged;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

attribute!(Regen, f32);

/// Triggers counted from the moment the effect is applied. `CurrentValueChanged` is not `Clone`, so
/// the harness' `record` cannot hold it.
#[derive(Resource, Default, Debug)]
struct Fired(pub usize);

/// Registers its attribute in the one place the contract says not to.
struct LatePlugin;

impl Plugin for LatePlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        init_attribute::<Regen>(app);
    }
}

#[test]
fn an_attribute_registered_in_finish_applies_nothing() {
    let mut app = app();
    app.add_plugins(LatePlugin);
    ticks(&mut app, 1);

    assert!(
        app.world()
            .resource::<AppTypeRegistry>()
            .read()
            .get(std::any::TypeId::of::<Regen>())
            .is_none(),
        "App::update() ran Plugin::finish, which the contract says only the app runner does"
    );

    let hero = spawn(&mut app, hero_def().with::<Regen>(4.0));
    let buff = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Regen>(5.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    app.init_resource::<Fired>();
    app.add_observer(
        |_: On<CurrentValueChanged<Regen>>, mut fired: ResMut<Fired>| {
            fired.0 += 1;
        },
    );

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 2);

    assert_eq!(
        with_effects(&mut app, move |effects| effects.on(hero)).len(),
        1,
        "the applied effect still spawns its modifier entity"
    );
    assert_eq!(base::<Regen>(&app, hero), 4.0, "the base never moves");
    assert_eq!(
        current::<Regen>(&app, hero),
        4.0,
        "and neither does the current value"
    );
    assert_eq!(
        app.world().resource::<Fired>().0,
        0,
        "no CurrentValueChanged is triggered"
    );
}
