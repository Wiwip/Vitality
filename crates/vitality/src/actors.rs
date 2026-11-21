use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;
use express_it::expr::{AsExpression, Expr};
use std::any::TypeId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use vitality_abilities::{Ability, AbilityDef, AbilityOf, GrantedAbilities};
use vitality_core::attributes::{Attribute, Clamp, ClampBounds, ManageAttributes};
use vitality_core::context::ActorExprSchema;
use vitality_core::mutator::EntityActions;
use vitality_core::registry::add_clamp_edges;
use vitality_effects::definition::EffectDef;
use vitality_effects::graph::NodeType;
use vitality_effects::{ApplyEffectEvent, EffectTargeting};

/// Per dependency key read by a clamp: the clamped attributes, so the resolver orders them last.
type ClampDependents = HashMap<TypeId, Vec<TypeId>>;

#[derive(Asset, TypePath)]
pub struct ActorDef {
    pub name: String,
    pub description: String,
    pub builder_actions: VecDeque<EntityActions>,
    pub abilities: Vec<Handle<AbilityDef>>,
    pub effects: Vec<vitality_core::store::Handle<EffectDef>>,

    pub clamp_dependents: ClampDependents,
}

#[derive(Component, Clone, Debug, Deref)]
#[require(ManageAttributes, GrantedAbilities)]
pub struct Actor(pub Handle<ActorDef>);

pub struct SpawnActorCommand {
    pub handle: Handle<ActorDef>,
}

impl EntityCommand for SpawnActorCommand {
    type Out = ();

    fn apply(self, mut entity: EntityWorldMut) {
        debug!("Spawning actor {} {:?}", entity.id(), self.handle);
        let actor_entity = entity.id();

        entity.world_scope(|world| {
            world.resource_scope(|world, actor_assets: Mut<Assets<ActorDef>>| {
                let Some(actor_def) = actor_assets.get(&self.handle) else {
                    warn!(
                        "Actor {:?} has no loaded definition; {actor_entity} left empty. Load the \
                         ActorDef into Assets<ActorDef> before spawning it",
                        self.handle
                    );
                    return;
                };

                let mut queue = {
                    let mut queue = CommandQueue::default();
                    let mut commands = Commands::new(&mut queue, world);

                    commands.entity(actor_entity).insert((
                        NodeType::Actor,
                        Actor(self.handle.clone()),
                        Name::new(actor_def.name.clone()),
                    ));

                    for actions in &actor_def.builder_actions {
                        let mut entity_commands = commands.entity(actor_entity);
                        (actions.func)(&mut entity_commands);
                    }

                    for ability in actor_def.abilities.iter() {
                        commands.spawn((
                            Ability {
                                handle: { ability.clone() },
                            },
                            AbilityOf(actor_entity),
                        ));
                    }

                    queue
                };

                register_clamp_edges(world, actor_entity, actor_def);

                // a starting effect reads the actor, so its attributes have to be on it already
                queue.apply(world);

                for effect in actor_def.effects.iter() {
                    world.trigger(ApplyEffectEvent {
                        entity: actor_entity,
                        targeting: EffectTargeting::SelfCast(actor_entity),
                        handle: *effect,
                    });
                }
            });
        });
    }
}

/// Clamped attributes read their limits' attributes, so those resolve first. A limit that reads
/// several attributes reports the rest of them later, from the resolver's first pass. A spawn marks
/// every attribute of the actor, so an edge that is new here needs no re-mark.
fn register_clamp_edges(world: &mut World, actor: Entity, def: &ActorDef) {
    add_clamp_edges(
        world,
        def.clamp_dependents
            .iter()
            .flat_map(|(key, clamped)| clamped.iter().map(move |clamped| (actor, *key, *clamped))),
    );
}

pub struct ActorBuilder {
    actor: ActorDef,
}

impl Default for ActorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ActorBuilder {
    pub fn new() -> ActorBuilder {
        Self {
            actor: ActorDef {
                name: "Actor".to_string(),
                description: "".to_string(),
                builder_actions: Default::default(),
                abilities: vec![],
                effects: vec![],
                clamp_dependents: Default::default(),
            },
        }
    }

    pub fn name(mut self, name: &str) -> ActorBuilder {
        self.actor.name = name.to_string();
        self
    }

    pub fn with<T: Attribute>(mut self, value: T::Property) -> ActorBuilder {
        self.actor.builder_actions.push_back(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.insert(T::new(value));
            },
        ));
        self
    }

    pub fn with_effect(mut self, effect: &vitality_core::store::Handle<EffectDef>) -> ActorBuilder {
        self.actor.effects.push(*effect);
        self
    }

    pub fn clamp<T>(
        mut self,
        min: impl AsExpression<T::Property, ActorExprSchema> + Send + Sync + 'static,
        max: impl AsExpression<T::Property, ActorExprSchema> + Send + Sync + 'static,
    ) -> ActorBuilder
    where
        T: Attribute,
    {
        let min_expr = min.as_expr();
        let max_expr = max.as_expr();

        let mut deps = HashSet::default();
        min_expr.get_dependencies(&mut deps);
        max_expr.get_dependencies(&mut deps);
        for dep in deps {
            self.actor
                .clamp_dependents
                .entry(dep)
                .or_default()
                .push(TypeId::of::<T>());
        }

        let bounds = ClampBounds::<T> {
            min: Arc::new(min_expr),
            max: Arc::new(max_expr),
        };
        self.actor.builder_actions.push_back(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.insert((Clamp::<T>::new(), bounds.clone()));
            },
        ));

        self
    }

    pub fn insert<T: Bundle + Clone + 'static>(mut self, bundle: T) -> ActorBuilder {
        self.actor.builder_actions.push_back(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.insert(bundle.clone());
            },
        ));
        self
    }

    pub fn grant_ability(mut self, ability_spec: &Handle<AbilityDef>) -> Self {
        self.actor.abilities.push(ability_spec.clone());
        self
    }

    pub fn build(self) -> ActorDef {
        self.actor
    }
}
