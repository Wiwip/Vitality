//! An apply whose target carries the attribute folds and says nothing, and an effect half installed
//! on its own; own binary for the `warn_once!`s.

mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use bevy::prelude::*;
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

#[test]
fn an_effect_attribute_installed_on_its_own_names_the_core_registration_it_lacks() {
    let logs = Logs::scoped();

    logs.run(|| {
        // AttributesPlugin registers Stacks, not Mana: this is the app that skipped init_attribute
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
        vitality::effect::init_effect_attribute::<Mana>(&mut app);
    });

    let warns = logs.warns();
    assert!(
        warns.contains("init_core_attribute::<Mana>"),
        "a fold installing on nothing has to name the attribute and its missing core half, got: {}",
        logs.all()
    );
}
