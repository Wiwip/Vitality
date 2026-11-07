use crate::view::{ActorRead, ActorWrite};
use crate::{EffectsMut, EffectsRef};
use bevy::ecs::world::{FilteredEntityMut, FilteredEntityRef};
use express_it::expr::{Context, ContextMut};

pub struct Source;
pub struct Target;
pub struct Caster;
pub struct EffectRole;

// A trait for contexts that have a "Source" or primary actor
pub trait ActorProvider<'a, 'b, Role> {
    type ActorRef: ActorRead;
    fn get_actor(&self) -> &Self::ActorRef;
}
pub trait ActorProviderMut<'a, 'b, Role> {
    type ActorMut: ActorWrite;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut;
}

/*********************************************************
 * Actor
 *********************************************************/
pub struct ActorExprSchema;
impl Context for ActorExprSchema {
    type ContextItem<'w, 's> = ActorExprContext<'w, 's>;
}
impl ContextMut for ActorExprSchema {
    type ContextItemMut<'w, 's> = ActorExprContextMut<'w, 's>;

    fn as_readonly<'w, 's, 'a>(
        mut_item: &'a Self::ContextItemMut<'w, 's>,
    ) -> Self::ContextItem<'a, 's> {
        ActorExprContext {
            actor_context: mut_item.actor_context.as_readonly(),
        }
    }
}

pub struct ActorExprContext<'w, 's> {
    pub actor_context: FilteredEntityRef<'w, 's>,
}
pub struct ActorExprContextMut<'w, 's> {
    pub actor_context: FilteredEntityMut<'w, 's>,
}

impl<'w, 's> ActorProvider<'w, 's, Source> for ActorExprContext<'w, 's> {
    type ActorRef = FilteredEntityRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.actor_context
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Source> for ActorExprContextMut<'w, 's> {
    type ActorMut = FilteredEntityMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.actor_context
    }
}

/*********************************************************
 * Effect
 *********************************************************/
pub struct EffectExprSchema;
impl Context for EffectExprSchema {
    type ContextItem<'w, 's> = EffectExprContext<'w, 's>;
}
impl ContextMut for EffectExprSchema {
    type ContextItemMut<'w, 's> = EffectExprContextMut<'w, 's>;

    fn as_readonly<'w, 's, 'a>(
        mut_item: &'a Self::ContextItemMut<'w, 's>,
    ) -> Self::ContextItem<'a, 's> {
        EffectExprContext {
            source_actor: mut_item.source_actor.as_readonly(),
            target_actor: mut_item.target_actor.as_ref().map(|v| v.as_readonly()),
            effect_holder: mut_item.effect_holder.as_readonly(),
        }
    }
}

pub struct EffectExprContext<'w, 's> {
    pub source_actor: EffectsRef<'w, 's>,
    pub target_actor: Option<EffectsRef<'w, 's>>,
    pub effect_holder: EffectsRef<'w, 's>,
}

pub struct EffectExprContextMut<'w, 's> {
    pub source_actor: EffectsMut<'w, 's>,
    pub target_actor: Option<EffectsMut<'w, 's>>,
    pub effect_holder: EffectsMut<'w, 's>,
}

impl<'w, 's> ActorProvider<'w, 's, Source> for EffectExprContext<'w, 's> {
    type ActorRef = EffectsRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.source_actor
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Source> for EffectExprContextMut<'w, 's> {
    type ActorMut = EffectsMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.source_actor
    }
}
impl<'w, 's> ActorProvider<'w, 's, Target> for EffectExprContext<'w, 's> {
    type ActorRef = EffectsRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        if let Some(target) = &self.target_actor {
            target
        } else {
            &self.source_actor
        }
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Target> for EffectExprContextMut<'w, 's> {
    type ActorMut = EffectsMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        if let Some(target) = &mut self.target_actor {
            target
        } else {
            &mut self.source_actor
        }
    }
}
impl<'w, 's> ActorProvider<'w, 's, EffectRole> for EffectExprContext<'w, 's> {
    type ActorRef = EffectsRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.effect_holder
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, EffectRole> for EffectExprContextMut<'w, 's> {
    type ActorMut = EffectsMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.effect_holder
    }
}
