use crate::Ability;
use crate::context::{AbilityExprContext, AbilityExprSchema};
use crate::definition::AbilityDef;
use bevy_asset::AssetId;
use bevy_ecs::prelude::Component;
use bevy_reflect::Reflect;
use express_it::expr::{DepKey, Deps, Expr};
use std::any::TypeId;
use std::fmt::Formatter;
use vitality_effects::conditions::HasComponent;
use vitality_effects::modifier::EffectSubject;

impl<C: Component + Reflect> Expr<bool, AbilityExprSchema> for HasComponent<C> {
    fn render(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "has {} on {:?}", self.who, std::any::type_name::<C>())
    }

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

    fn get_dependencies(&self, deps: &mut Deps) {
        deps.insert(DepKey::Type(TypeId::of::<C>()));
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
    fn get_dependencies(&self, _deps: &mut Deps) {}

    fn render(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Is Ability {}", self.asset)
    }
}

impl std::fmt::Debug for IsAbility {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Is Ability {}", self.asset)
    }
}
