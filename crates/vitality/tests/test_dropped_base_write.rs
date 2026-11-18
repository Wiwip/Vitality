//! The base write of an attribute the actor does not carry: dropped, and it says so.
//!
//! The cell sits apart from `test_modify_base.rs`, which owns the same `warn_once!` call site in
//! `modify_base`: that flag fires once for the whole binary, so two cells writing a missing
//! attribute would race for the one record.

mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use bevy::log::tracing::subscriber::with_default;
use bevy::log::tracing_subscriber::{Registry, layer::SubscriberExt};
use gameplay::*;
use vitality::actors::ActorBuilder;

#[test]
fn a_base_write_on_an_attribute_the_actor_lacks_is_dropped_with_a_warn() {
    let logs = Logs::default();
    let subscriber = Registry::default().with(logs.layer());

    let mute = with_default(subscriber, || {
        let mut app = app();
        let mute = spawn(
            &mut app,
            ActorBuilder::new().name("Mute").with::<Health>(10.0),
        );
        with_vitality(&mut app, move |v| v.set_base::<Mana>(mute, 25.0));
        ticks(&mut app, 1);

        assert!(
            !has::<Mana>(&app, mute),
            "the dropped write must not conjure the component"
        );
        assert_eq!(base::<Health>(&app, mute), 10.0, "nothing else moves");
        mute
    });

    let warns = logs.warns();
    let logs = logs.all();
    assert!(
        warns.contains("Mana")
            && warns.contains(".with::<Mana>()")
            && warns.contains("init_attribute::<Mana>()"),
        "the dropped base write has to warn at WARN level and name both remedies, got: {logs}"
    );
    assert!(
        warns.contains(&mute.to_string()),
        "and name the actor the write was aimed at, got: {logs}"
    );
}
