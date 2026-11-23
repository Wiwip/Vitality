//! The app-level attribute rules at the facade: the cap sentence, the collision hint, late registration.

mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use bevy::prelude::*;
use std::any::TypeId;
use vitality::attributes::ReflectAccessAttribute;
use vitality::effect::Stacks;
use vitality::{AttributesPlugin, attribute, init_attribute};
use vitality_core::registry::{AttrSet, AttributeRegistry};

macro_rules! cap_pool {
    ($( $name:ident ),+ $(,)?) => {
        $( attribute!($name); )+

        /// One entry per type, so a test registers a prefix of the pool instead of spelling it out.
        static GAME_ATTRIBUTES: &[fn(&mut App)] = &[$(init_attribute::<$name>),+];
    };
}

cap_pool!(
    Attr0, Attr1, Attr2, Attr3, Attr4, Attr5, Attr6, Attr7, Attr8, Attr9, Attr10, Attr11, Attr12,
    Attr13, Attr14, Attr15, Attr16, Attr17, Attr18, Attr19, Attr20, Attr21, Attr22, Attr23, Attr24,
    Attr25, Attr26, Attr27, Attr28, Attr29, Attr30, Attr31, Attr32, Attr33, Attr34, Attr35, Attr36,
    Attr37, Attr38, Attr39, Attr40, Attr41, Attr42, Attr43, Attr44, Attr45, Attr46, Attr47, Attr48,
    Attr49, Attr50, Attr51, Attr52, Attr53, Attr54, Attr55, Attr56, Attr57, Attr58, Attr59, Attr60,
    Attr61, Attr62, Attr63, Attr64, Attr65, Attr66, Attr67, Attr68, Attr69, Attr70, Attr71, Attr72,
    Attr73, Attr74, Attr75, Attr76, Attr77, Attr78, Attr79, Attr80, Attr81, Attr82, Attr83, Attr84,
    Attr85, Attr86, Attr87, Attr88, Attr89, Attr90, Attr91, Attr92, Attr93, Attr94, Attr95, Attr96,
    Attr97, Attr98, Attr99, Attr100, Attr101, Attr102, Attr103, Attr104, Attr105, Attr106, Attr107,
    Attr108, Attr109, Attr110, Attr111, Attr112, Attr113, Attr114, Attr115, Attr116, Attr117,
    Attr118, Attr119, Attr120, Attr121, Attr122, Attr123, Attr124, Attr125, Attr126, Attr127
);

/// never ticked: the cap is a build-time counter
fn app_with(game: usize) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    for register in &GAME_ATTRIBUTES[..game] {
        register(&mut app);
    }
    app
}

#[test]
fn an_app_gets_127_game_attributes_because_the_framework_holds_one_slot() {
    let app = app_with(AttrSet::CAPACITY - 1);
    let registry = app.world().resource::<AttributeRegistry>();

    assert!(
        registry.id_of(TypeId::of::<Stacks>()).is_some(),
        "AttributesPlugin registers the framework's own Stacks, and that is the slot the sentence counts"
    );
    assert_eq!(
        registry.len(),
        AttrSet::CAPACITY,
        "{} game attributes plus the framework's own attribute is an app at the cap, not one short \
         of it: the registry holds {}",
        AttrSet::CAPACITY - 1,
        registry.len(),
    );
}

/// `catch_unwind` cannot read the payload in this profile, so `should_panic` matches the message
#[test]
#[should_panic(
    expected = "at most 128 attributes and that cap does not grow; merge two of the attributes you register"
)]
fn the_128th_game_attribute_is_the_one_the_app_refuses() {
    let _app = app_with(AttrSet::CAPACITY);
}

mod game_a {
    use vitality::prelude::*;
    vitality::attribute!(Health, f32);
    vitality::attribute!(Armor, f32);
}

mod game_b {
    use vitality::prelude::*;
    vitality::attribute!(Health, f32);
    vitality::attribute!(Mana, f32);
}

/// called, not passed to `add_plugins`: two `fn(&mut App)` values share one plugin type id
fn app_of_crates(game: &[fn(&mut App)]) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    for register in game {
        register(&mut app);
    }
    app
}

#[test]
#[should_panic(expected = "already holds the short name: rename one of them")]
fn two_crates_sharing_a_short_name_are_told_to_rename_one() {
    let _shared = app_of_crates(&[
        init_attribute::<game_a::Health>,
        init_attribute::<game_b::Health>,
    ]);
}

/// The wiring contract's first rule from the bad side: a `Plugin::finish` registration never happens
mod late_registration {

    use crate::gameplay::*;
    use bevy::prelude::*;
    use vitality::CurrentValueChanged;
    use vitality::effect::modifier::{EffectSubject, ModOp};
    use vitality::prelude::*;

    attribute!(Regen, f32);

    /// the event is not `Clone`, so the harness' `record` cannot hold it
    #[derive(Resource, Default, Debug)]
    struct Fired(pub usize);

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
}

/// The other half of late registration: a registration made while the app ticks moves the registry
/// out from under the resolver, which copies it for the whole pass loop.
mod registered_after_the_first_tick {

    use crate::gameplay::*;
    use bevy::prelude::*;
    use vitality::CurrentValueChanged;
    use vitality::effect::modifier::{EffectSubject, ModOp};
    use vitality::prelude::*;

    attribute!(Late, f32);

    /// the event is not `Clone`, so the harness' `record` cannot hold it
    #[derive(Resource, Default)]
    struct Fired(pub usize);

    #[test]
    fn an_attribute_registered_after_the_first_tick_still_notifies() {
        let mut app = app();
        // one real fixed tick: that is what builds the resolver's copy of the registry
        ticks(&mut app, 1);
        init_attribute::<Late>(&mut app);
        app.init_resource::<Fired>();
        app.add_observer(
            |_: On<CurrentValueChanged<Late>>, mut fired: ResMut<Fired>| {
                fired.0 += 1;
            },
        );

        // no tick in between: the resolver's first pass after the registration is the one that moves Late
        let mut def = Some(hero_def().with::<Late>(4.0).build());
        let hero = with_vitality(&mut app, move |vitality| {
            vitality.add_spawn_actor(def.take().expect("run once")).id()
        });
        let buff = effect(
            &mut app,
            EffectBuilder::permanent()
                .modify::<Late>(5.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );
        apply(&mut app, hero, hero, &buff);
        ticks(&mut app, 2);

        assert_eq!(current::<Late>(&app, hero), 9.0, "the late attribute folds");
        assert_eq!(
            app.world().resource::<Fired>().0,
            1,
            "the observer sees the change"
        );
    }
}

/// The instant half of the same rule: the queue the deferred write rides needs a registered attribute
mod unregistered_instant {

    use crate::gameplay::*;
    use crate::logs::Logs;
    use vitality::effect::modifier::{EffectSubject, ModOp};
    use vitality::prelude::*;

    attribute!(Stray, f32);

    #[test]
    fn an_instant_effect_on_an_unregistered_attribute_is_dropped_with_a_warn() {
        let logs = Logs::scoped();

        logs.run(|| {
            let mut app = app();
            let hero = spawn(&mut app, hero_def().with::<Stray>(4.0));
            let buff = effect(
                &mut app,
                EffectBuilder::instant()
                    .modify::<Stray>(5.0, ModOp::Add, EffectSubject::Target)
                    .build(),
            );

            apply(&mut app, hero, hero, &buff);
            ticks(&mut app, 2);

            assert_eq!(base::<Stray>(&app, hero), 4.0, "the base never moves");
            assert_eq!(
                current::<Stray>(&app, hero),
                4.0,
                "and neither does the current value"
            );
        });

        // warn_once! is per call site, so this cell asserts the text and never a count
        let warns = logs.warns();
        let logs = logs.all();
        assert!(
            logs.contains("Stray") && logs.contains("init_attribute"),
            "the dropped instant write has to name the attribute and the call that registers it, got: {logs}"
        );
        assert!(
            warns.contains("Stray"),
            "a write the app cannot queue is a warning, not a debug note, got: {warns}"
        );
    }
}
