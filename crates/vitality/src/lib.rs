//! Gameplay attributes, effects and abilities for Bevy.
//!
//! # Getting started
//!
//! The README's scenario, run: register the attributes and the tag, build the actor with
//! [`ActorBuilder`](crate::actors::ActorBuilder), apply a burn that scales with `Power`. With
//! [`TimeUpdateStrategy`](bevy::time::TimeUpdateStrategy)::`ManualDuration` one `update` is one
//! fixed tick, so the update count is the number of seconds.
//!
//! ```
//! use bevy::ecs::system::RunSystemOnce;
//! use bevy::prelude::*;
//! use bevy::time::TimeUpdateStrategy;
//! use std::time::Duration;
//! use vitality::actors::ActorBuilder;
//! use vitality::effect::modifier::{EffectSubject, ModOp};
//! use vitality::prelude::*;
//! use vitality::{attribute, tag};
//!
//! attribute!(Health, f32);
//! attribute!(MaxHealth, f32);
//! attribute!(Power, f32);
//! tag!(Burning);
//!
//! // bevy's default fixed timestep
//! const STEP: Duration = Duration::from_micros(15_625);
//! const TICKS_PER_SECOND: usize = 64;
//!
//! let mut app = App::new();
//! app.add_plugins((
//!     MinimalPlugins,
//!     AssetPlugin::default(),
//!     VitalityPlugin::new()
//!         .attribute::<Health>()
//!         .attribute::<MaxHealth>()
//!         .attribute::<Power>(),
//! ))
//! .insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
//!
//! // the first update primes the fixed clock and runs no tick of it
//! app.update();
//!
//! let hero = app
//!     .world_mut()
//!     .run_system_once(|mut vitality: Vitality| {
//!         vitality
//!             .add_spawn_actor(
//!                 ActorBuilder::new()
//!                     .name("Hero")
//!                     .with::<Health>(100.0)
//!                     .with::<MaxHealth>(100.0)
//!                     .with::<Power>(10.0)
//!                     .clamp::<Health>(0.0, MaxHealth::src())
//!                     .build(),
//!             )
//!             .id()
//!     })
//!     .unwrap();
//!
//! // the apply must be a later system: the spawn is a command; no actor exists before the flush
//! app.update();
//! let burn = app
//!     .world_mut()
//!     .run_system_once(|mut vitality: Vitality| {
//!         vitality.add_effect(
//!             EffectBuilder::every_second_for_duration(1.0, 5.0)
//!                 .with_tag::<Burning>()
//!                 .modify::<Health>(Power::src(), ModOp::Sub, EffectSubject::Target)
//!                 .build(),
//!         )
//!     })
//!     .unwrap();
//! app
//!     .world_mut()
//!     .run_system_once(move |mut vitality: Vitality| vitality.apply_effect_to_self(hero, &burn))
//!     .unwrap();
//!
//! let health = |app: &App| app.world().get::<Health>(hero).unwrap().current_value();
//! let burning = |app: &App| app.world().get::<Burning>(hero).is_some();
//!
//! for _ in 0..TICKS_PER_SECOND {
//!     app.update();
//! }
//! assert_eq!(health(&app), 90.0, "one fire a second, one Power each");
//! assert!(burning(&app), "the hero carries Burning while the burn lasts");
//!
//! for _ in 0..(4 * TICKS_PER_SECOND) {
//!     app.update();
//! }
//! assert_eq!(health(&app), 50.0, "five fires in the five seconds, not one more");
//! assert!(!burning(&app), "the tag goes with the effect");
//! ```
//!
//! # Wiring a plugin
//!
//! [`AttributesPlugin`] owns the app-wide state everything else reads and [`init_attribute`] writes
//! into it, and both are `build`-time. The resolver mode is read at that same moment, which is why
//! the order below is a contract rather than a style choice. Two of the five rules fail silently;
//! the other three abort the build with a message that names the cause.
//!
//! - Register attributes in `Plugin::build`, not `Plugin::finish`. `App::update()` never runs
//!   `Plugin::finish` — only `App::finish()` does, and that is the app runner's job (`App::run`,
//!   `ScheduleRunnerPlugin`), so an app driven by `update()` (every test harness) never registers
//!   the attribute at all. Silently: an effect applied to it still spawns its modifier entity, but
//!   the base never moves and no [`CurrentValueChanged`] is triggered. Registering after the app has
//!   ticked does not repair actors that already hold modifiers either: they stay at the unmodified
//!   value until the next thing that writes one of their attributes or applies another effect.
//! - The 128-attribute cap is one counter for the whole app, not one per crate: the registry is a
//!   single resource and it refuses the 129th registration by name
//!   (`cannot register X: the attribute registry holds at most 128 attributes`). Two plugins with 64
//!   attributes each already trip it — the 128th game attribute is the one refused, because the
//!   framework's own [`effect::Stacks`] attribute holds a slot, so an app gets 127.
//! - An `AttrId` is `infos.len()` at the moment of registration, i.e. a registration index, so
//!   reordering two `add_plugins` calls or deleting one attribute renumbers the rest. Never persist
//!   one; persist the name the registry records for the attribute (`AttrInfo::name`). That name is
//!   the type's *short* name — the module path is dropped — so it must be unique across every crate
//!   in the app, and the app enforces it for you: registering `game_a::Health` and
//!   `game_b::Health` panics with `Attribute type ID collision for Health`.
//! - Add [`AttributesPlugin`] before any plugin that calls [`init_attribute`]. That one is loud: the
//!   registry does not exist yet, so the app dies with
//!   `resource does not exist: vitality_core::registry::AttributeRegistry`.
//! - The DAG schedule nodes are added once, by [`effect::EffectsPlugin`], and only if
//!   [`ResolverMode::Dag`] is in the world when its `build` runs (`vitality_effects/src/lib.rs:95-125`):
//!   `resolve_system`, `apply_all_modifier_events`, `scan_base_writes` and
//!   `apply_all_periodic_effects` are the only apply-path nodes the DAG ever gets, because
//!   [`effect::init_effect_attribute`] adds no node and no observer for an attribute in DAG mode.
//!   Two ways out of that contract are silent. An app that installs attributes with
//!   `init_attribute`/`init_effect_attribute` and never adds `EffectsPlugin` has no `EffectRegistry`
//!   and no listener for the apply event, so `Effects::apply` quietly does nothing. So does one that
//!   runs `EffectsPlugin::build` under [`ResolverMode::PerAttribute`] and registers its attributes
//!   under [`ResolverMode::Dag`]: add `AttributesPlugin` while `PerAttribute` is in the world, insert
//!   `Dag`, then register with [`init_attribute`]. Those attributes get neither apply path —
//!   [`effect::EffectApplied`] fires and the modifier entity exists while the base never moves, no
//!   [`CurrentValueChanged`] is triggered, and a periodic effect ticks without ever firing. The
//!   reverse order is not that case: `Dag` at `EffectsPlugin::build` with the attributes registered
//!   under `PerAttribute` gives a mixed-mode app — the framework's own attributes keep the collapsed
//!   DAG nodes, the ones registered later take the per-attribute path — and it applies and fires like
//!   a plain per-attribute app while paying both paths: the mode-blind `scan_base_writes` walks them
//!   every tick, `resolve_system` walks them whenever anything is queued, `mark_actor_dirty_dag` is a
//!   global observer, and nothing orders `resolve_system` against
//!   `update_current_value_system::<T>`. It runs; nothing here pins it. Inserting a mode once the
//!   plugins are all built changes nothing — the mode is read only at
//!   `EffectsPlugin::build`, at `init_effect_attribute` (`vitality_effects/src/lib.rs:95`, `:144`)
//!   and in the clamp branch of `init_attribute`, all of them build time. `ResolverMode::default()`
//!   is already `Dag`, so the rule is one mode for the whole build — [`VitalityPlugin::resolver`]
//!   inserts it before the plugins.
//!
//! Whether the contract stays "add `EffectsPlugin`, and pick one mode before it" or
//! `init_effect_attribute` grows a first-install guard that installs the collapsed nodes itself is
//! still the owner's call (D-52). Neither case is reachable in this repo: [`AttributesPlugin`] adds
//! [`effect::EffectsPlugin`] in its `build`, so the first needs an app that registers attributes
//! straight through `vitality_core`/`vitality_effects` instead, and nothing here inserts a mode after
//! a build — [`VitalityPlugin::resolver`] inserts it before the plugins, and both in-repo
//! `insert_resource(ResolverMode)` sites (`tests/test_registration_cost.rs:228`,
//! `benches/support/common.rs:327`) sit before their `add_plugins`.
//!
//! ```
//! use bevy::prelude::*;
//! use vitality::prelude::*;
//! use vitality::{AttributesPlugin, init_attribute};
//!
//! attribute!(Health, f32);
//!
//! struct MyGamePlugin;
//!
//! impl Plugin for MyGamePlugin {
//!     fn build(&self, app: &mut App) {
//!         // AttributesPlugin owns the registry init_attribute writes into, so it goes first
//!         app.add_plugins(AttributesPlugin);
//!         app.add_plugins(init_attribute::<Health>);
//!     }
//! }
//!
//! let mut app = App::new();
//! app.add_plugins((MinimalPlugins, AssetPlugin::default(), MyGamePlugin));
//!
//! assert!(
//!     app.world()
//!         .resource::<AppTypeRegistry>()
//!         .read()
//!         .get(std::any::TypeId::of::<Health>())
//!         .is_some(),
//!     "registration is a build-time effect, so the attribute is already there"
//! );
//! ```
//!
//! The duplicate short name of the third rule, pinned. This form cannot name the message, so the
//! example builds the same two modules twice: two names is fine, one name twice is not.
//!
//! ```should_panic
//! use bevy::prelude::*;
//! use vitality::prelude::*;
//! use vitality::{AttributesPlugin, init_attribute};
//!
//! // the registry keys on the short name, so two crates shipping a Health is one name
//! mod game_a {
//!     use vitality::prelude::*;
//!     vitality::attribute!(Health, f32);
//!     vitality::attribute!(Armor, f32);
//! }
//! mod game_b {
//!     use vitality::prelude::*;
//!     vitality::attribute!(Health, f32);
//!     vitality::attribute!(Mana, f32);
//! }
//!
//! let mut app = App::new();
//! app.add_plugins((
//!     MinimalPlugins,
//!     AssetPlugin::default(),
//!     AttributesPlugin,
//!     init_attribute::<game_a::Health>,
//!     init_attribute::<game_b::Mana>,
//! ));
//!
//! let mut app = App::new();
//! app.add_plugins((
//!     MinimalPlugins,
//!     AssetPlugin::default(),
//!     AttributesPlugin,
//!     init_attribute::<game_a::Health>,
//!     init_attribute::<game_b::Health>,
//! ));
//! ```

use crate::actors::{ActorDef, ActorRegistry};
use crate::clamp::{Clamp, apply_clamps, clamp_dependency_changed};
use bevy::prelude::*;
use vitality_abilities::AbilityPlugin;
use vitality_core::CorePlugin;
use vitality_core::attributes::Attribute;
use vitality_core::init_core_attribute;
use vitality_core::schedule::EffectsSet;
pub use vitality_effects::ResolverMode;
use vitality_effects::{
    ConditionPlugin, EffectsPlugin, GlobalEffectPlugin, Stacks, init_effect_attribute,
};

pub use vitality_abilities as ability;
pub use vitality_core::{
    AppAttributeBindings, AttributeBindings, AttributeError, BaseValueChanged, CurrentValueChanged,
    attribute, attribute_impl, attributes, express_it, math, mutator, num_traits, tag, tags,
};
pub use vitality_effects as effect;

pub mod actors;
pub mod clamp;
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
    pub use vitality_core::util::{pretty_type_name, pretty_type_name_str};
    pub use vitality_debug::*;
}

/// Everything vitality needs except the attributes themselves. See [`VitalityPlugin`] for a
/// version that also registers attributes, and [the crate page][crate] for the wiring
/// contract this plugin's `build` is part of.
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
        app.add_plugins((
            CorePlugin,
            AbilityPlugin,
            ConditionPlugin,
            EffectsPlugin,
            GlobalEffectPlugin,
        ))
        .init_asset::<ActorDef>()
        .insert_resource(ActorRegistry::default())
        .add_plugins(init_attribute::<Stacks>);
    }
}

/// [`AttributesPlugin`] plus the attributes this game uses, and the resolver mode it runs.
///
/// ```
/// use bevy::prelude::*;
/// use vitality::prelude::*;
/// use vitality::{ResolverMode, VitalityPlugin};
///
/// attribute!(Health, f32);
/// attribute!(Mana, f32);
///
/// let mut app = App::new();
/// app.add_plugins((
///     MinimalPlugins,
///     AssetPlugin::default(),
///     // `resolver` inserts the mode before `build` adds the plugins, so every build-time read of
///     // it — `EffectsPlugin`, `init_effect_attribute`, `init_attribute` — sees the same one
///     VitalityPlugin::new()
///         .resolver(ResolverMode::Dag)
///         .attribute::<Health>()
///         .attribute::<Mana>(),
/// ));
/// ```
#[derive(Default)]
pub struct VitalityPlugin {
    attributes: Vec<fn(&mut App)>,
    resolver: Option<ResolverMode>,
}

impl VitalityPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolver(mut self, mode: ResolverMode) -> Self {
        self.resolver = Some(mode);
        self
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
        if let Some(mode) = self.resolver {
            app.insert_resource(mode);
        }
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

    app.register_type::<Clamp<T>>();
    // the DAG resolver evaluates clamps inside its walk
    if vitality_effects::resolver_mode(app) == ResolverMode::Dag {
        return;
    }
    app.add_systems(
        FixedUpdate,
        apply_clamps::<T>
            .after(vitality_effects::systems::update_current_value_system::<T>)
            .in_set(EffectsSet::UpdateCurrentValues),
    );
    app.add_observer(clamp_dependency_changed::<T>);
}
