//! An apply whose target carries the attribute the effect moves: it folds, and nothing is said.
//!
//! Its own binary because the `warn_once!` in `ApplyEffectEvent::context` is one record per call
//! site per binary: sharing it with the cell that expects the record would let whichever cell ran
//! second pass without the warn ever being printed.

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
fn an_apply_to_an_entity_carrying_the_attribute_says_nothing() {
    let logs = Logs::default();
    let subscriber = Registry::default().with(logs.layer());

    let post = with_default(subscriber, || {
        let mut app = app();
        let post = app.world_mut().spawn(Health::new(100.0)).id();
        let blessing = effect(
            &mut app,
            EffectBuilder::permanent()
                .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );
        apply(&mut app, post, post, &blessing);
        ticks(&mut app, 1);

        assert_eq!(
            current::<Health>(&app, post),
            110.0,
            "the fold landed, so there was something to fold into"
        );
        post
    });

    let warns = logs.warns();
    assert!(
        !warns.contains("no registered attribute"),
        "an entity that carries the attribute the effect moves has something to fold into, got: {}",
        logs.all()
    );
    assert!(
        !warns.contains(&post.to_string()),
        "and nothing is warned about it, got: {}",
        logs.all()
    );
}
