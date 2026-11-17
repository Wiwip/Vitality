#![allow(dead_code)]

//! Game-shaped workloads for the scaling question: does a tick stay inside budget at thousands of
//! actors? Every workload drives the library only through what a game would touch (base writes,
//! effect application and expiry, spawns), so the same id measures any resolver behind it.

use crate::common::{
    ATTRIBUTE_POOL, WorldSpec, actor_entities, build_app_at, build_label, seed_rng, tick,
};
use bevy::prelude::*;
use rand::RngExt;
use rand::rngs::StdRng;
use std::collections::VecDeque;
use vitality::actors::{Actor, SpawnActorCommand};
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::effect::{ApplyEffectEvent, EffectStackingPolicy, EffectTargeting};
use vitality::prelude::*;
use vitality::tags::ChangeTag;

use crate::common::{Attr0, Attr1, Attr2, Attr3, Attr4, Attr5, Attr6, Attr7, Attr8, Attr9};

pub const ATTRIBUTES: usize = 20;
pub const EFFECTS_PER_ACTOR: usize = 3;
/// Settling ticks after the build, so the startup fold is not in the first sample.
pub const SETTLE_TICKS: usize = 4;
/// High enough that no run reaches it: `stack_add` measures the add path, never a clamped no-op.
const STACK_CAP: u32 = u32::MAX;

vitality::tag!(Burning);
vitality::tag!(Immune);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// Nothing changes: the floor.
    Idle,
    /// 10 % of the actors get every attribute's base moved by one each tick.
    Steady,
    /// Every actor, every attribute, every tick.
    Storm,
    /// Every attribute rewritten with the value it already has.
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
    /// A permanent buff re-applied to 10 % of the actors per tick, each hit adding a stack.
    StackAdd,
    /// A buff on a fixed 10 % re-applied every tick, so the refresh is what keeps it up.
    StackRefresh,
    /// An instant combo on 20 % of the actors per tick that only lands on the tagged half.
    TagRequired,
    /// The same instant combo, blocked on the half the immunity tag covers.
    Immunity,
    /// A buff on 10 % of the actors per tick, sized from a value captured at apply.
    Snapshot,
    /// A DoT ticking on a fixed 20 %, re-applied every tick so the periodic fire is what is measured.
    Dot,
}

impl Workload {
    pub const ALL: [Workload; 15] = [
        Workload::Idle,
        Workload::Steady,
        Workload::Storm,
        Workload::NoOpWrite,
        Workload::Damage,
        Workload::Aura,
        Workload::BuffChurn,
        Workload::Combat,
        Workload::Spawn,
        Workload::StackAdd,
        Workload::StackRefresh,
        Workload::TagRequired,
        Workload::Immunity,
        Workload::Snapshot,
        Workload::Dot,
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
            Workload::StackAdd => "stack_add",
            Workload::StackRefresh => "stack_refresh",
            Workload::TagRequired => "tag_required",
            Workload::Immunity => "immunity",
            Workload::Snapshot => "snapshot",
            Workload::Dot => "dot",
        }
    }
}

pub fn scenario_id(workload: Workload, actors: usize) -> String {
    format!(
        "scenario/{}/actors={actors}/a={ATTRIBUTES}/e={EFFECTS_PER_ACTOR}/{}",
        workload.name(),
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
    /// The actors `StackRefresh` hits every tick, so one instance is the one being refreshed.
    window: Vec<Entity>,
}

pub fn build(workload: Workload, actors: usize) -> ScenarioWorld {
    build_in(workload, actors)
}

/// [`build`] under the name the guard tests use.
pub fn build_in(workload: Workload, actors: usize) -> ScenarioWorld {
    let spec = WorldSpec {
        attributes_per_actor: ATTRIBUTES,
        effects_per_actor: EFFECTS_PER_ACTOR,
        modifiers_per_effect: 1,
        actors,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    };
    let mut app = build_app_at(&spec, ATTRIBUTE_POOL);
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
        window: Vec::new(),
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
        Workload::StackAdd => {
            world.effect = Some(add_effect(
                &mut world.app,
                EffectBuilder::permanent()
                    .modify::<Attr4>(2.0, ModOp::Add, EffectSubject::Target)
                    .with_stacking_policy(EffectStackingPolicy::Add {
                        count: 1,
                        max_stack: STACK_CAP,
                    })
                    .build(),
            ));
        }
        Workload::StackRefresh => {
            let timestep = world.app.world().resource::<Time<Fixed>>().timestep();
            world.window = actor_list[..actor_list.len() / 10].to_vec();
            // outlives the settle but not a gap in the refresh
            world.effect = Some(add_effect(
                &mut world.app,
                EffectBuilder::for_seconds(timestep.as_secs_f32() * (SETTLE_TICKS as f32 + 3.0))
                    .modify::<Attr5>(3.0, ModOp::Add, EffectSubject::Target)
                    .with_stacking_policy(EffectStackingPolicy::RefreshDuration)
                    .build(),
            ));
            let handle = world.effect.clone().expect("just added");
            let window = world.window.clone();
            world.queue_apply(&handle, &window);
            world.app.world_mut().flush();
        }
        Workload::TagRequired => {
            grant_every_other::<Burning>(&mut world.app, &actor_list);
            world.effect = Some(add_effect(
                &mut world.app,
                EffectBuilder::instant()
                    .requires_tag::<Burning>()
                    .modify::<Attr6>(-2.0, ModOp::Add, EffectSubject::Target)
                    .build(),
            ));
        }
        Workload::Immunity => {
            grant_every_other::<Immune>(&mut world.app, &actor_list);
            world.effect = Some(add_effect(
                &mut world.app,
                EffectBuilder::instant()
                    .blocked_by_tag::<Immune>()
                    .modify::<Attr7>(-2.0, ModOp::Add, EffectSubject::Target)
                    .build(),
            ));
        }
        Workload::Snapshot => {
            let timestep = world.app.world().resource::<Time<Fixed>>().timestep();
            world.effect = Some(add_effect(
                &mut world.app,
                // settle plus one tick, so a timed run sees it applied and then lapsing
                EffectBuilder::for_seconds(timestep.as_secs_f32() * (SETTLE_TICKS as f32 + 1.0))
                    .modify_snapshot::<Attr8>(Attr1::src(), ModOp::Add, EffectSubject::Target)
                    .build(),
            ));
        }
        Workload::Dot => {
            let timestep = world.app.world().resource::<Time<Fixed>>().timestep();
            world.window = actor_list[..actor_list.len() / 5].to_vec();
            // under one timestep, so a ticker is due on every tick the sample runs
            world.effect = Some(add_effect(
                &mut world.app,
                EffectBuilder::every_second_permanently(timestep.as_secs_f32() * 0.5)
                    .modify::<Attr9>(-1.0, ModOp::Add, EffectSubject::Target)
                    .with_stacking_policy(EffectStackingPolicy::RefreshDuration)
                    .build(),
            ));
            let handle = world.effect.clone().expect("just added");
            let window = world.window.clone();
            world.queue_apply(&handle, &window);
            world.app.world_mut().flush();
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

/// Grants `T` to every other actor: a tag rule splits the world, both halves keep taking applies.
fn grant_every_other<T: Component + Default>(app: &mut App, actors: &[Entity]) {
    let world = app.world_mut();
    for &actor in actors.iter().step_by(2) {
        let mut commands = world.commands();
        commands
            .entity(actor)
            .queue_silenced(ChangeTag::grant(TagSpec::of::<T>()));
    }
    world.flush();
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
                let targets: Vec<Entity> = (0..n / share)
                    .map(|_| self.actors[self.rng.random_range(0..n)])
                    .collect();
                self.queue_apply(&handle, &targets);
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
            }
            Workload::StackAdd | Workload::Snapshot => {
                let handle = self.effect.clone().expect("workload has an effect");
                let window = self.round_robin(n / 10);
                self.queue_apply(&handle, &window);
            }
            Workload::StackRefresh | Workload::Dot => {
                let handle = self.effect.clone().expect("workload has an effect");
                let window = self.window.clone();
                self.queue_apply(&handle, &window);
            }
            Workload::TagRequired | Workload::Immunity => {
                let handle = self.effect.clone().expect("workload has an effect");
                let window = self.round_robin(n / 5);
                self.queue_apply(&handle, &window);
            }
        }
    }

    fn round_robin(&mut self, count: usize) -> Vec<Entity> {
        let n = self.actors.len();
        let picked: Vec<Entity> = (0..count)
            .map(|i| self.actors[(self.cursor + i) % n])
            .collect();
        self.cursor += count;
        picked
    }

    /// Queues one self-targeted apply per actor: `World::trigger` dispatches its observers inline, so
    /// only a command-queue trigger leaves the tag test, condition eval and stacking for `tick` to time.
    fn queue_apply(&mut self, handle: &Handle<EffectDef>, targets: &[Entity]) {
        let world = self.app.world_mut();
        let mut commands = world.commands();
        for &target in targets {
            commands.trigger(ApplyEffectEvent {
                entity: target,
                targeting: EffectTargeting::new(target, target),
                handle: handle.clone(),
            });
        }
    }

    /// The timed unit: one fixed tick, preceded by the flush that decides *where* this tick's input
    /// lands. bevy drains the command queue after every exclusive system and `resolve_system` is one, so
    /// the applies land either way and deleting this flush keeps every guard green; what the flush pins is
    /// the place — the input is in before any of the tick's systems run, not mid-tick at whatever
    /// exclusive system drains the queue first. Realism, not a measurement boundary.
    pub fn tick(&mut self) {
        self.app.world_mut().flush();
        tick(&mut self.app);
    }
}
