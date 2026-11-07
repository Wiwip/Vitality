use crate::{AbilityMut, AbilityRef};
use express_it::expr::{Context, ContextMut};
use vitality_core::context::{ActorProvider, ActorProviderMut, Caster, Source, Target};

pub struct AbilityRole;

/*********************************************************
 * Ability
 *********************************************************/
pub struct AbilityExprSchema;
impl Context for AbilityExprSchema {
    type ContextItem<'w, 's> = AbilityExprContext<'w, 's>;
}
impl ContextMut for AbilityExprSchema {
    type ContextItemMut<'w, 's> = AbilityExprContextMut<'w, 's>;

    fn as_readonly<'w, 's, 'a>(
        mut_item: &'a Self::ContextItemMut<'w, 's>,
    ) -> Self::ContextItem<'a, 's> {
        AbilityExprContext {
            caster_ref: mut_item.caster_mut.as_readonly(),
            ability_ref: mut_item.ability_mut.as_readonly(),
            target_ref: mut_item.target_mut.as_ref().map(|v| v.as_readonly()),
        }
    }
}

pub struct AbilityExprContext<'w, 's> {
    pub caster_ref: AbilityRef<'w, 's>,
    pub target_ref: Option<AbilityRef<'w, 's>>,
    pub ability_ref: AbilityRef<'w, 's>,
}

pub struct AbilityExprContextMut<'w, 's> {
    pub caster_mut: AbilityMut<'w, 's>,
    pub target_mut: Option<AbilityMut<'w, 's>>,
    pub ability_mut: AbilityMut<'w, 's>,
}

impl<'w, 's> ActorProvider<'w, 's, Caster> for AbilityExprContext<'w, 's> {
    type ActorRef = AbilityRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.caster_ref
    }
}
impl<'w, 's> ActorProvider<'w, 's, Source> for AbilityExprContext<'w, 's> {
    type ActorRef = AbilityRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.caster_ref
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Caster> for AbilityExprContextMut<'w, 's> {
    type ActorMut = AbilityMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.caster_mut
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Source> for AbilityExprContextMut<'w, 's> {
    type ActorMut = AbilityMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.caster_mut
    }
}

impl<'w, 's> ActorProvider<'w, 's, Target> for AbilityExprContext<'w, 's> {
    type ActorRef = AbilityRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        if let Some(target) = &self.target_ref {
            target
        } else {
            &self.caster_ref
        }
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Target> for AbilityExprContextMut<'w, 's> {
    type ActorMut = AbilityMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        if let Some(target) = &mut self.target_mut {
            target
        } else {
            &mut self.caster_mut
        }
    }
}

impl<'w, 's> ActorProvider<'w, 's, AbilityRole> for AbilityExprContext<'w, 's> {
    type ActorRef = AbilityRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.ability_ref
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, AbilityRole> for AbilityExprContextMut<'w, 's> {
    type ActorMut = AbilityMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.ability_mut
    }
}
