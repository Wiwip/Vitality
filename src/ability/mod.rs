mod builder;
mod command;
mod system_param;
mod systems;

use crate::ability::systems::{
    activate_ability, reset_ability_cooldown, tick_ability_cooldown, try_activate_ability_observer,
};
use crate::assets::AbilityDef;
use crate::condition::{HasComponent, IsAbility};
use crate::schedule::EffectsSet;
use bevy::prelude::*;
pub use builder::AbilityBuilder;
pub use command::GrantAbilityCommand;
use express_it::expr::Expr;
use express_it::logic::{BoolExpr, BoolExprNode};
use std::error::Error;
use std::fmt::Formatter;
use std::sync::Arc;
pub use system_param::Abilities;
use crate::context::AbilityExprSchema;
use crate::prelude::EffectExprSchema;

pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, tick_ability_cooldown.in_set(EffectsSet::Prepare))
            .add_observer(try_activate_ability_observer)
            .add_observer(reset_ability_cooldown)
            .add_observer(activate_ability)
            .register_type::<AbilityOf>()
            .register_type::<GrantedAbilities>();
    }
}

/// The entity that this effect is targeting.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = GrantedAbilities)]
pub struct AbilityOf(pub Entity);

/// All abilities granted to this entity.
#[derive(Component, Reflect, Debug, Default)]
#[relationship_target(relationship = AbilityOf, linked_spawn)]
pub struct GrantedAbilities(Vec<Entity>);

impl GrantedAbilities {
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.0.iter()
    }
}

#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct Ability(pub(crate) Handle<AbilityDef>);

#[derive(EntityEvent)]
pub struct TryActivateAbility {
    #[event_target]
    ability: Entity,
    condition: BoolExpr<AbilityExprSchema>,
    target_data: TargetData,
}

impl TryActivateAbility {
    pub fn by_tag<T: Component + Reflect>(target: Entity, target_data: TargetData) -> Self {
        let node = BoolExprNode::Boxed(Box::new(HasComponent::<T>::effect()));
        let expr = Expr::new(Arc::new(node));

        Self {
            ability: target,
            condition: expr,
            target_data,
        }
    }
    pub fn by_def(target: Entity, handle: AssetId<AbilityDef>, target_data: TargetData) -> Self {
        let node = BoolExprNode::Boxed(Box::new(IsAbility::new(handle)));
        let expr = Expr::new(Arc::new(node));

        Self {
            ability: target,
            condition: expr,
            target_data,
        }
    }
}

#[derive(Component)]
pub struct AbilityCooldown {
    pub(crate) timer: Timer,
    pub(crate) value: Expr<f64, EffectExprSchema>,
}

#[derive(Debug)]
pub enum TargetData {
    SelfCast,
    Target(Entity),
}

#[derive(EntityEvent)]
pub struct BeginAbility {
    pub source: Entity,
    #[event_target]
    pub ability: Entity,
}

#[derive(EntityEvent)]
pub struct ExecuteAbility {
    #[event_target]
    pub ability: Entity,
    pub target: Entity,
    pub source: Entity,
}

#[derive(EntityEvent)]
pub struct EndAbility {
    #[event_target]
    pub ability: Entity,
    pub source: Entity,
}

#[derive(EntityEvent)]
pub struct AbilityCancel {
    #[event_target]
    pub ability: Entity,
    pub source: Entity,
}

#[derive(Clone, Debug)]
pub enum AbilityError {
    GrantingAbilityToNonActor(Entity),
    AbilityDoesNotExist(Entity),
}

impl std::fmt::Display for AbilityError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AbilityError::GrantingAbilityToNonActor(entity) => {
                write!(
                    f,
                    "{}: Cannot grant ability to entities that are not actors. with TypeId  not present on entity.",
                    entity
                )
            }
            AbilityError::AbilityDoesNotExist(entity) => {
                write!(
                    f,
                    "{}: The entity is not an ability (e.g. No Ability component).",
                    entity
                )
            }
        }
    }
}

impl Error for AbilityError {}
