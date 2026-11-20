//! An apply whose target carries the attribute the effect moves: it folds, and nothing is said.
//!
//! Its own binary because the `warn_once!` in `ApplyEffectEvent::context` is one record per call
//! site per binary: sharing it with the cell that expects the record would let whichever cell ran
//! second pass without the warn ever being printed.

mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

#[test]
fn an_apply_to_an_entity_carrying_the_attribute_says_nothing() {
    let logs = Logs::scoped();

    let post = logs.run(|| {
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
