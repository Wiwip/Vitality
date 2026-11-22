//! Gameplay attributes, effects and abilities for Bevy: one attribute registry, an effect runtime
//! that writes modifiers into it, and an ability state machine on top. [`VitalityPlugin`] wires the
//! three; the example below is the README's scenario, run.
//!
//! ```
//! use bevy::ecs::system::RunSystemOnce;
//! use bevy::prelude::*;
//! use bevy::time::TimeUpdateStrategy;
//! use std::time::Duration;
//! use vitality::{actors::ActorBuilder, attribute, effect::modifier::{EffectSubject, ModOp}, prelude::*, tag};
//!
//! attribute!(Health, f32); attribute!(MaxHealth, f32); attribute!(Power, f32); tag!(Burning);
//!
//! // bevy's default fixed timestep, so with ManualDuration one update is one tick
//! const STEP: Duration = Duration::from_micros(15_625);
//! const TICKS_PER_SECOND: usize = 64;
//!
//! let mut app = App::new();
//! app.add_plugins((MinimalPlugins, AssetPlugin::default(), VitalityPlugin::new()
//!     .attribute::<Health>().attribute::<MaxHealth>().attribute::<Power>()))
//!     .insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
//! // the first update primes the fixed clock and runs no tick of it
//! app.update();
//!
//! let hero = app.world_mut().run_system_once(|mut vitality: Vitality| vitality.add_spawn_actor(
//!     ActorBuilder::new().name("Hero")
//!         .with::<Health>(100.0).with::<MaxHealth>(100.0).with::<Power>(10.0)
//!         .clamp::<Health>(0.0, MaxHealth::src()).build()).id()).unwrap();
//! // one tick of separation: the spawn flushed with its system, so an apply there would have landed too
//! app.update();
//!
//! let burn = app.world_mut().run_system_once(|mut vitality: Vitality| vitality.add_effect(
//!     EffectBuilder::every_second_for_duration(1.0, 5.0).with_tag::<Burning>()
//!         .modify::<Health>(Power::src(), ModOp::Sub, EffectSubject::Target).build())).unwrap();
//! app.world_mut().run_system_once(move |mut vitality: Vitality|
//!     vitality.apply_effect_to_self(hero, &burn)).unwrap();
//!
//! let health = |app: &App| app.world().get::<Health>(hero).unwrap().current_value();
//! let burning = |app: &App| app.world().get::<Burning>(hero).is_some();
//! for _ in 0..TICKS_PER_SECOND { app.update(); }
//! assert_eq!(health(&app), 90.0, "one fire a second, one Power each");
//! assert!(burning(&app), "the hero carries Burning while the burn lasts");
//! for _ in 0..(4 * TICKS_PER_SECOND) { app.update(); }
//! assert_eq!(health(&app), 50.0, "five fires in the five seconds, not one more");
//! assert!(!burning(&app), "the tag goes with the effect");
//! ```
//!
//! Wiring it by hand is six rules.
//! - Register attributes in `Plugin::build`, never `Plugin::finish`: `App::update()` never runs `finish`, so the attribute is never registered and nothing applied to it lands.
//! - The 128-attribute cap is one counter for the whole app and the framework's own [`effect::Stacks`] holds a slot, so an app gets 127 game attributes and the 128th is the one refused by name.
//! - An `AttrId` is `infos.len()` when it is handed out, so reordering or deleting a registration renumbers the rest: persist the `AttrInfo::name` the registry records instead — it drops the module path, so two crates sharing a short name panic the app.
//! - Add [`AttributesPlugin`] before any plugin that calls [`init_attribute`], or the app dies at build with `resource does not exist: vitality_core::registry::AttributeRegistry`.
//! - [`effect::EffectsPlugin`] adds the apply-path nodes once for the app (`resolve_system`, `apply_all_modifier_events`, `scan_base_writes`, `apply_all_periodic_effects`) and [`effect::init_effect_attribute`] adds an attribute's effect-side state with no node of its own, so an attribute installed without it leaves `Effects::apply` doing nothing; keep `apply_all_periodic_effects` ahead of `tick_effect_durations` or the expiry despawn wins the tick and the fire due on it is lost.
//! - Both actors of an instant apply have to outlive it: the apply only queues the write and `EffectsSet::UpdateBaseValues` lands it up to a tick later, where a `Live` expression such as `Power::src()` is evaluated again, so a source that despawned in between drops the write, while a `modify_snapshot` write lands the value the apply captured and needs nobody.
//!
//! [`actors`] turns an [`ActorBuilder`](actors::ActorBuilder) into an
//! [`ActorDef`] that [`context::Vitality`] spawns and reads, [`attributes::Clamp`] is the
//! min/max component an actor carries, [`registry`] holds the effect and ability registries, and
//! [`effect`] and [`ability`] are the runtime crates behind them.
//! `crates/vitality/examples/` has whole apps.

use crate::actors::ActorDef;
use bevy_app::prelude::*;
use bevy_asset::AssetApp;
use vitality_abilities::AbilityPlugin;
use vitality_core::CorePlugin;
use vitality_core::attributes::{Attribute, Clamp};
use vitality_core::init_core_attribute;
use vitality_effects::{
    ConditionPlugin, EffectsPlugin, GlobalEffectPlugin, Stacks, init_effect_attribute,
};

pub use vitality_abilities as ability;
pub use vitality_core::{
    AppAttributeBindings, AttributeBindings, AttributeError, BaseValueChanged, CurrentValueChanged,
    attribute, attribute_impl, attribute_set, attributes, express_it, mutator, store, tag, tags,
};
pub use vitality_effects as effect;

pub mod actors;
pub mod registry;
mod vitality;

pub mod context {
    pub use crate::vitality::Vitality;
    pub use vitality_core::context::*;
}

pub mod prelude {
    pub use crate::context::Vitality;
    pub use crate::{AttributesPlugin, VitalityPlugin, init_attribute};
    pub use vitality_abilities::prelude::*;
    pub use vitality_abilities::{
        Abilities, AbilityActivated, AbilityActivationFailed, AbilityEnded, ActivationError,
    };
    pub use vitality_core::tags::{ActorTags, TagSpec};
    pub use vitality_effects::prelude::*;
    pub use vitality_effects::{
        EffectApplied, EffectBlocked, EffectRemovalReason, EffectRemoved, EffectStacksChanged,
        Effects,
    };
}

#[cfg(feature = "debug-overlay")]
pub mod inspector {
    pub use vitality_debug::*;
}

/// Everything but the attributes themselves; see [`VitalityPlugin`] and [the crate page][crate].
#[derive(Default)]
pub struct AttributesPlugin;

impl Plugin for AttributesPlugin {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !vitality_core::first_install::<Self>(app) {
            return;
        }
        // abilities and actor defs still use scenes; DefaultPlugins may have added it already
        if !app.is_plugin_added::<bevy_scene::ScenePlugin>() {
            app.add_plugins(bevy_scene::ScenePlugin);
        }
        app.add_plugins((
            CorePlugin,
            AbilityPlugin,
            ConditionPlugin,
            EffectsPlugin,
            GlobalEffectPlugin::default(),
        ))
        .init_asset::<ActorDef>()
        .add_plugins(init_attribute::<Stacks>);
    }
}

/// [`AttributesPlugin`] plus the attributes this game uses; the [crate page][crate] builds one.
#[derive(Default)]
pub struct VitalityPlugin {
    attributes: Vec<fn(&mut App)>,
}

impl VitalityPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attribute<T: Attribute>(mut self) -> Self {
        self.attributes.push(init_attribute::<T>);
        self
    }
}

impl Plugin for VitalityPlugin {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        app.add_plugins(AttributesPlugin);
        for register in &self.attributes {
            register(app);
        }
    }
}

struct AttributeInstalled<T>(std::marker::PhantomData<T>);

/// Registers one attribute. Safe to call more than once for the same `T`.
pub fn init_attribute<T: Attribute>(app: &mut App) {
    if !vitality_core::first_install::<AttributeInstalled<T>>(app) {
        return;
    }
    init_core_attribute::<T>(app);
    init_effect_attribute::<T>(app);

    // the resolver evaluates clamps inside its walk, so a clamp costs no schedule node of its own
    app.register_type::<Clamp<T>>();
}
