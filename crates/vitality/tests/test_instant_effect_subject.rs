//! An instant effect spawns no effect entity, so a modifier aimed at `EffectSubject::Effect` has
//! nowhere to write: dropped, and it says so and names the remedy.
//!
//! The cell sits apart from `test_cross_actor.rs`, which reaches the same `warn_once!` call site in
//! `apply_modifier`: that flag fires once for the whole binary, so two cells would race for the one
//! record.

mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::EffectBuilder;

#[test]
fn an_instant_effect_subject_write_is_dropped_and_names_instant_as_the_reason() {
    let logs = Logs::scoped();

    logs.run(|| {
        let mut app = app();
        let caster = spawn(&mut app, hero_def());
        let target = spawn(&mut app, hero_def());
        let hex = effect(
            &mut app,
            EffectBuilder::instant()
                .modify::<Health>(25.0, ModOp::Add, EffectSubject::Effect)
                .build(),
        );
        apply(&mut app, caster, target, &hex);
        ticks(&mut app, 2);
    });

    // warn_once! is per call site, so this matches text and never a count
    let warns = logs.warns();
    let logs = logs.all();
    assert!(
        warns.contains("Mod<Health>")
            && warns.contains("instant")
            && warns.contains("EffectSubject::Target")
            && warns.contains("persistent"),
        "the dropped write has to warn at WARN level and name the instant effect and the remedy, \
         got: {logs}"
    );
}
