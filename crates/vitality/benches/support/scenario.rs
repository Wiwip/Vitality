#![allow(dead_code)]

//! Game-shaped workloads for the scaling question: does a tick stay inside budget at thousands of
//! actors? Every workload drives the library only through what a game would touch (base writes,
//! effect application and expiry, spawns), so the same id measures any resolver behind it.
//!
//! The per-tick input runs untimed in [`ScenarioWorld::drive`]; the timed part is one
//! `app.update()`, which is exactly one fixed tick under the harness clock.

use crate::common::{
    ATTRIBUTE_POOL, WorldSpec, actor_entities, build_label, resolver_label, seed_rng, tick,
};
use bevy::prelude::*;
use rand::RngExt;
use rand::rngs::StdRng;
use std::collections::VecDeque;
use vitality::actors::{Actor, SpawnActorCommand};
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::effect::{ApplyEffectEvent, Effect, EffectTargeting};
use vitality::prelude::*;

use crate::common::{Attr0, Attr1, Attr2, Attr3};

/// Attributes per actor in every scenario; the owner's target shape.
pub const ATTRIBUTES: usize = 20;
pub const EFFECTS_PER_ACTOR: usize = 3;
/// Settling ticks after the build, so the startup fold is not in the first sample.
pub const SETTLE_TICKS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// Nothing changes: the floor.
    Idle,
    /// 10 % of the actors get every attribute's base moved by one each tick.
    Steady,
    /// Every actor, every attribute, every tick (BENCH-PLAN D: `churn/dirty_storm`).
    Storm,
    /// Every attribute rewritten with the value it already has (BENCH-PLAN D: `churn/no_op_write`).
    NoOpWrite,
    /// 10 % of the actors take a hit on one attribute.
    Damage,
    /// One caster's live aura on up to 1000 targets, the caster's stat moving every tick.
    Aura,
    /// Short buffs applied to 10 % of the actors per tick, each expiring after three ticks.
    BuffChurn,
    /// Instant damage effects on 20 % of the actors per tick.
    Combat,
    /// 5 % of the actors despawned and as many spawned per tick.
    Spawn,
}

impl Workload {
    pub const ALL: [Workload; 9] = [
        Workload::Idle,
        Workload::Steady,
        Workload::Storm,
        Workload::NoOpWrite,
        Workload::Damage,
        Workload::Aura,
        Workload::BuffChurn,
        Workload::Combat,
        Workload::Spawn,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Workload::Idle => "idle",
            Workload::Steady => "steady",
            Workload::Storm => "storm",
            Workload::NoOpWrite => "no_op_write",
            Workload::Damage => "damage",
            Workload::Aura => "aura",
            Workload::BuffChurn => "buff_churn",
            Workload::Combat => "combat",
            Workload::Spawn => "spawn",
        }
    }
}

pub fn scenario_id(workload: Workload, actors: usize) -> String {
    format!(
        "scenario/{}/actors={actors}/a={ATTRIBUTES}/e={EFFECTS_PER_ACTOR}/{}/{}",
        workload.name(),
        resolver_label(),
        build_label()
    )
}

pub struct ScenarioWorld {
    pub app: App,
    pub workload: Workload,
    pub actors: Vec<Entity>,
    rng: StdRng,
    cursor: usize,
    tick: u64,
    caster: Option<Entity>,
    effect: Option<Handle<EffectDef>>,
    spawned: VecDeque<Entity>,
}

pub fn build(workload: Workload, actors: usize) -> ScenarioWorld {
    let spec = WorldSpec {
        attributes_per_actor: ATTRIBUTES,
        effects_per_actor: EFFECTS_PER_ACTOR,
        modifiers_per_effect: 1,
        actors,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    };
    let mut app = spec.build_app();
    let actor_list = actor_entities(app.world_mut());
    let mut world = ScenarioWorld {
        app,
        workload,
        actors: actor_list.clone(),
        rng: seed_rng(),
        cursor: 0,
        tick: 0,
        caster: None,
        effect: None,
        spawned: actor_list.iter().copied().collect(),
    };

    match workload {
        Workload::Aura => {
            let caster = actor_list[0];
            let aura = add_effect(
                &mut world.app,
                EffectBuilder::permanent()
                    .modify::<Attr1>(Attr0::src(), ModOp::Add, EffectSubject::Target)
                    .build(),
            );
            for &target in actor_list.iter().skip(1).take(1000) {
                world.app.world_mut().trigger(ApplyEffectEvent {
                    entity: target,
                    targeting: EffectTargeting::new(caster, target),
                    handle: aura.clone(),
                });
            }
            world.app.world_mut().flush();
            world.caster = Some(caster);
        }
        Workload::BuffChurn => {
            let timestep = world.app.world().resource::<Time<Fixed>>().timestep();
            world.effect = Some(add_effect(
                &mut world.app,
                EffectBuilder::for_seconds(timestep.as_secs_f32() * 3.0)
                    .modify::<Attr2>(1.0, ModOp::Add, EffectSubject::Target)
                    .build(),
            ));
        }
        Workload::Combat => {
            world.effect = Some(add_effect(
                &mut world.app,
                EffectBuilder::instant()
                    .modify::<Attr3>(-1.0, ModOp::Add, EffectSubject::Target)
                    .build(),
            ));
        }
        _ => {}
    }

    for _ in 0..SETTLE_TICKS {
        tick(&mut world.app);
    }
    world
}

fn add_effect(app: &mut App, def: EffectDef) -> Handle<EffectDef> {
    app.world_mut().resource_mut::<Assets<EffectDef>>().add(def)
}

impl ScenarioWorld {
    /// The untimed input of one tick.
    pub fn drive(&mut self) {
        self.tick += 1;
        let n = self.actors.len();
        let attrs = &ATTRIBUTE_POOL[..ATTRIBUTES];
        match self.workload {
            Workload::Idle => {}
            Workload::Steady | Workload::Storm => {
                let count = if self.workload == Workload::Storm {
                    n
                } else {
                    n / 10
                };
                let delta = if self.tick % 2 == 0 { 1.0 } else { -1.0 };
                for _ in 0..count {
                    let actor = self.actors[self.cursor % n];
                    self.cursor += 1;
                    let world = self.app.world_mut();
                    for attr in attrs {
                        if let Some(value) = attr.base(world, actor) {
                            attr.set_base(world, actor, value + delta);
                        }
                    }
                }
            }
            Workload::NoOpWrite => {
                let world = self.app.world_mut();
                for &actor in &self.actors {
                    for attr in attrs {
                        if let Some(value) = attr.base(world, actor) {
                            attr.set_base(world, actor, value);
                        }
                    }
                }
            }
            Workload::Damage => {
                let attr = &attrs[0];
                for _ in 0..n / 10 {
                    let actor = self.actors[self.cursor % n];
                    self.cursor += 1;
                    let world = self.app.world_mut();
                    if let Some(value) = attr.base(world, actor) {
                        attr.set_base(world, actor, value - 1.0);
                    }
                }
            }
            Workload::Aura => {
                let caster = self.caster.expect("aura has a caster");
                let delta = if self.tick % 2 == 0 { 1.0 } else { -1.0 };
                let world = self.app.world_mut();
                let value = attrs[0].base(world, caster).unwrap_or(0.0);
                attrs[0].set_base(world, caster, value + delta);
            }
            Workload::BuffChurn | Workload::Combat => {
                let share = if self.workload == Workload::Combat {
                    5
                } else {
                    10
                };
                let handle = self.effect.clone().expect("workload has an effect");
                let world = self.app.world_mut();
                for _ in 0..n / share {
                    let target = self.actors[self.rng.random_range(0..n)];
                    world.trigger(ApplyEffectEvent {
                        entity: target,
                        targeting: EffectTargeting::new(target, target),
                        handle: handle.clone(),
                    });
                }
                world.flush();
            }
            Workload::Spawn => {
                let count = (n / 20).max(1);
                let handle = {
                    let world = self.app.world_mut();
                    let mut query = world.query::<&Actor>();
                    query.iter(world).next().map(|actor| actor.0.clone())
                };
                let Some(handle) = handle else {
                    return;
                };
                let world = self.app.world_mut();
                for _ in 0..count {
                    if let Some(old) = self.spawned.pop_front() {
                        world.despawn(old);
                    }
                    let mut commands = world.commands();
                    let mut entity = commands.spawn_empty();
                    entity.queue(SpawnActorCommand {
                        handle: handle.clone(),
                    });
                    self.spawned.push_back(entity.id());
                }
                world.flush();
            }
        }
    }

    pub fn tick(&mut self) {
        tick(&mut self.app);
    }

    pub fn effect_count(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&Effect>().iter(world).count()
    }

    pub fn actor_count(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&Actor>().iter(world).count()
    }
}
