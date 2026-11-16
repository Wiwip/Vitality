//! `Vitality::modify_base` is the base-write entry point. This pins what it does today: every op
//! lands on the base, the current value follows in one fixed tick, the write rides a
//! `CurrentValueChanged`, and an attribute the actor does not carry is dropped without a trace.
//! Effects stay out of it on purpose — a live dependent reading what you write is D-56's ground.

mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use vitality::CurrentValueChanged;
use vitality::actors::ActorBuilder;
use vitality::attributes::Attribute;
use vitality::effect::modifier::ModOp;

/// One entry per `CurrentValueChanged<T>` trigger seen, as `(old, new)`. The event is not `Clone`,
/// so the harness' `record` cannot hold it.
#[derive(Resource, Default)]
struct Fires(Vec<(f32, f32)>);

fn watch<T: Attribute<Property = f32>>(app: &mut App) {
    app.init_resource::<Fires>();
    app.add_observer(
        move |ev: On<CurrentValueChanged<T>>, mut fires: ResMut<Fires>| {
            fires.0.push((ev.event().old, ev.event().new));
        },
    );
}

fn fires(app: &App) -> Vec<(f32, f32)> {
    app.world().resource::<Fires>().0.clone()
}

/// One `modify_base`, then the single fixed tick the current value is given to follow.
fn write(app: &mut App, actor: Entity, op: ModOp, value: f32) {
    with_vitality(app, move |v| v.modify_base::<Health>(actor, op, value));
    ticks(app, 1);
}

#[test]
fn add_sub_and_set_land_on_the_base_and_the_current_value_follows_in_one_tick() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    watch::<Health>(&mut app);

    write(&mut app, hero, ModOp::Add, 25.0);
    assert_eq!(base::<Health>(&app, hero), 125.0, "Add moves the base");
    assert_eq!(
        current::<Health>(&app, hero),
        125.0,
        "the current value follows in the one tick"
    );

    write(&mut app, hero, ModOp::Sub, 15.0);
    assert_eq!(
        base::<Health>(&app, hero),
        110.0,
        "Sub subtracts from the base"
    );
    assert_eq!(current::<Health>(&app, hero), 110.0);

    write(&mut app, hero, ModOp::Set, 40.0);
    assert_eq!(base::<Health>(&app, hero), 40.0, "Set replaces the base");
    assert_eq!(current::<Health>(&app, hero), 40.0);

    assert_eq!(
        fires(&app),
        [(100.0, 125.0), (125.0, 110.0), (110.0, 40.0)],
        "one CurrentValueChanged per write, from the value the previous write left"
    );

    // writing the value the base already holds: the base does not move, and HEAD stays quiet
    write(&mut app, hero, ModOp::Set, 40.0);
    assert_eq!(base::<Health>(&app, hero), 40.0);
    assert_eq!(current::<Health>(&app, hero), 40.0);
    assert_eq!(
        fires(&app).len(),
        3,
        "a Set that moves nothing triggers nothing"
    );
}

#[test]
fn modify_base_on_an_attribute_the_actor_does_not_carry_changes_nothing() {
    let mut app = app();
    let mute = spawn(
        &mut app,
        ActorBuilder::new().name("Mute").with::<Health>(10.0),
    );
    watch::<Health>(&mut app);
    watch::<Armor>(&mut app);

    with_vitality(&mut app, move |v| {
        v.modify_base::<Armor>(mute, ModOp::Add, 5.0);
        v.modify_base::<Armor>(mute, ModOp::Sub, 5.0);
        v.modify_base::<Armor>(mute, ModOp::Set, 5.0);
    });
    ticks(&mut app, 1);

    assert!(!has::<Armor>(&app, mute), "no Armor appears on the actor");
    assert_eq!(
        base::<Health>(&app, mute),
        10.0,
        "nothing else moves either"
    );
    assert_eq!(current::<Health>(&app, mute), 10.0);
    assert!(
        fires(&app).is_empty(),
        "the dropped write says nothing: {:?}",
        fires(&app)
    );
}
