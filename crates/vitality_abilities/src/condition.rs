use crate::Ability;
use crate::context::{AbilityExprContext, AbilityExprSchema};
use crate::definition::AbilityDef;
use bevy::asset::AssetId;
use bevy::prelude::{Component, Reflect};
use express_it::expr::Expr;
use std::any::TypeId;
use std::collections::HashSet;
use std::fmt::Formatter;
use vitality_core::condition::HasComponent;
use vitality_core::modifier::EffectSubject;

impl<C: Component + Reflect> Expr<bool, AbilityExprSchema> for HasComponent<C> {
    fn eval(&self, ctx: &AbilityExprContext) -> bool {
        match self.who {
            EffectSubject::Target => match ctx.target_ref {
                Some(target) => target.get::<C>().is_some(),
                None => ctx.caster_ref.get::<C>().is_some(),
            },
            EffectSubject::Source => ctx.caster_ref.get::<C>().is_some(),
            EffectSubject::Effect => ctx.ability_ref.get::<C>().is_some(),
        }
    }

    fn get_dependencies(&self, deps: &mut HashSet<TypeId>) {
        deps.insert(TypeId::of::<C>());
    }
}

pub struct IsAbility {
    asset: AssetId<AbilityDef>,
}

impl IsAbility {
    pub fn new(asset: AssetId<AbilityDef>) -> Self {
        Self { asset }
    }
}

impl Expr<bool, AbilityExprSchema> for IsAbility {
    fn eval(&self, ctx: &AbilityExprContext) -> bool {
        let ability = match ctx.ability_ref.get::<Ability>() {
            Some(ability) => ability,
            _ => {
                return false;
            }
        };

        ability.handle.id() == self.asset
    }
    fn get_dependencies(&self, _deps: &mut HashSet<TypeId>) {}
}

impl std::fmt::Debug for IsAbility {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Is Ability {}", self.asset)
    }
}
