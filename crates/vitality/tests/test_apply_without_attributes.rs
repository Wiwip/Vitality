//! An apply whose target has nothing to fold into: no attribute of its own, and it says so.
//!
//! The cell sits apart from `test_apply_with_attributes.rs`: both hit the same `warn_once!` call
//! site in `ApplyEffectEvent::context`, and that flag fires once for the whole binary, so two cells
//! sharing it would race for the one record.

mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use bevy::log::tracing::subscriber::with_default;
use bevy::log::tracing_subscriber::{Registry, layer::SubscriberExt};
use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

#[test]
fn an_apply_to_an_entity_carrying_no_attribute_warns_that_there_is_nothing_to_fold_into() {
    let logs = Logs::default();
    let subscriber = Registry::default().with(logs.layer());

    let (prop, effect) = with_default(subscriber, || {
        let mut app = app();
        let prop = app.world_mut().spawn(()).id();
        let blessing = effect(
            &mut app,
            EffectBuilder::permanent()
                .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );
        let effect = blessing.id();
        apply(&mut app, prop, prop, &blessing);
        ticks(&mut app, 1);

        assert!(
            !has::<Health>(&app, prop),
            "the apply must not conjure the attribute it has nowhere to fold"
        );
        (prop, effect)
    });

    let warns = logs.warns();
    let logs = logs.all();
    assert!(
        warns.contains("no registered attribute")
            && warns.contains(&prop.to_string())
            && warns.contains(&effect.to_string()),
        "an apply to an entity with nothing to fold into has to warn at WARN level and name both \
         the effect and that entity, got: {logs}"
    );
}
