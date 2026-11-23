use crate::application::EffectApplicationPolicy;
use crate::conditions::IsAttributeWithinBounds;
use crate::context::EffectExprSchema;
use crate::definition::EffectDef;
use crate::modifier::{AttributeModifier, Capture, EffectSubject};
use crate::{EffectStackingPolicy, EffectTarget};
use bevy_ecs::prelude::{
    Bundle, ChildOf, Component, Entity, EntityCommands, EntityEvent, EntityWorldMut, Name, Observer,
};
use bevy_ecs::system::IntoObserverSystem;
use express_it::expr::{AsExpression, Expr};
use std::ops::RangeBounds;
use std::sync::Arc;
use tracing::warn;
use vitality_core::attributes::Attribute;
use vitality_core::modifier::ModOp;
use vitality_core::mutator::EntityActions;
use vitality_core::tags::TagSpec;
use vitality_core::util::pretty_type_name;

/// the observer belongs to the effect, so removal takes it along, while `anchor` says what it watches
fn effect_trigger<E: EntityEvent, M>(
    anchor: impl Fn(&EntityWorldMut) -> Option<Entity> + Send + Sync + 'static,
    observer: impl IntoObserverSystem<E, M> + Clone + Sync + 'static,
) -> EntityActions {
    let anchor = Arc::new(anchor);
    EntityActions::new(move |entity_commands: &mut EntityCommands| {
        let anchor = anchor.clone();
        let observer = observer.clone();
        entity_commands.queue(move |mut effect: EntityWorldMut| {
            let effect_id = effect.id();
            let Some(watched) = anchor(&effect) else {
                warn!("[{effect_id}] Effect has no anchor: its trigger watches nothing.");
                return;
            };
            let mut observer = Observer::new(observer);
            observer.watch_entity(watched);
            effect.world_scope(|world| {
                world.spawn((
                    ChildOf(effect_id),
                    observer,
                    Name::new(format!("On<{}>", pretty_type_name::<E>())),
                ));
            });
        });
    })
}

pub struct EffectBuilder {
    def: EffectDef,
}

impl EffectBuilder {
    pub fn new(application: EffectApplicationPolicy) -> Self {
        Self {
            def: EffectDef {
                application_policy: application,
                stacking_policy: EffectStackingPolicy::None,
                effect_fn: vec![],
                activate_conditions: vec![],
                attach_conditions: vec![],
                on_actor_triggers: vec![],
                on_effect_triggers: vec![],
                modifiers: vec![],
                granted_tags: vec![],
                required_tags: vec![],
                blocked_by_tags: vec![],
                cleanses: vec![],
            },
        }
    }

    pub fn instant() -> Self {
        Self::new(EffectApplicationPolicy::Instant)
    }

    pub fn permanent() -> Self {
        Self::new(EffectApplicationPolicy::Permanent)
    }

    /// an [`Self::active_while`] gate that misses pauses this duration too
    pub fn for_seconds(duration: f32) -> Self {
        Self::new(EffectApplicationPolicy::for_seconds(duration))
    }

    pub fn every_second_permanently(interval: f32) -> Self {
        Self::new(EffectApplicationPolicy::every_seconds(interval))
    }

    /// an [`Self::active_while`] gate that misses pauses the interval and this duration too
    pub fn every_second_for_duration(interval: f32, duration: f32) -> Self {
        Self::new(EffectApplicationPolicy::every_seconds_for_duration(
            interval, duration,
        ))
    }

    /// instant/periodic write `who`; a persistent effect folds into its carrier: `Source` writes its target
    ///
    /// # Example
    /// ```
    /// # use vitality_effects::prelude::*;
    /// attribute!(Health, u32);
    /// attribute!(Damage, u32);
    ///
    /// let effect = EffectBuilder::new(EffectApplicationPolicy::Instant)
    ///     .modify::<Health>(100u32, ModOp::Add, EffectSubject::Source)
    ///     .build();
    ///
    /// let damage = EffectBuilder::instant()
    ///     .modify::<Health>(Damage::src(), ModOp::Sub, EffectSubject::Target)
    ///     .build();
    ///
    /// let regen = EffectBuilder::every_second_for_duration(1.0, 12.0)
    ///     .modify::<Health>(2u32, ModOp::Add, EffectSubject::Source)
    ///     .build();
    /// ```
    pub fn modify<T: Attribute>(
        self,
        expr: impl AsExpression<T::Property, EffectExprSchema> + Send + Sync + 'static,
        op: ModOp,
        who: EffectSubject,
    ) -> Self {
        self.push_modifier::<T>(Arc::new(expr.as_expr()), op, who, Capture::Live)
    }

    /// like [`Self::modify`], but evaluated once at application, never following later attribute changes
    pub fn modify_snapshot<T: Attribute>(
        self,
        expr: impl AsExpression<T::Property, EffectExprSchema> + Send + Sync + 'static,
        op: ModOp,
        who: EffectSubject,
    ) -> Self {
        self.push_modifier::<T>(Arc::new(expr.as_expr()), op, who, Capture::Snapshot)
    }

    fn push_modifier<T: Attribute>(
        mut self,
        expr: Arc<dyn Expr<T::Property, EffectExprSchema> + Send + Sync>,
        operation: ModOp,
        who: EffectSubject,
        capture: Capture,
    ) -> Self {
        self.def.modifiers.push(Box::new(AttributeModifier::<T> {
            expr,
            value: T::Property::default(),
            who,
            folds_into_holder: false,
            operation,
            capture,
        }));
        self
    }

    /// read once at application, unlike [`Self::active_while`], and instant effects never read it at all; the apply has no effect entity yet, so an `Effect` subject reads the source
    ///
    /// # Example
    /// ```
    /// # use vitality_effects::prelude::*;
    /// # use vitality_effects::conditions::ChanceCondition;
    /// attribute!(Health, f32);
    /// attribute!(Damage, f32);
    ///
    /// EffectBuilder::every_second_for_duration(1.0, 12.0)
    ///     .modify::<Health>(Damage::src(), ModOp::Add, EffectSubject::Target)
    ///     .attach_if(ChanceCondition(0.10))
    ///     .build();
    /// ```
    pub fn attach_if(
        mut self,
        condition: impl Expr<bool, EffectExprSchema> + Send + Sync + 'static,
    ) -> Self {
        self.def.attach_conditions.push(Box::new(condition));
        self
    }

    /// while this misses the effect is inactive: folds nothing, fires nothing, duration paused until it holds
    pub fn active_while(
        mut self,
        condition: impl Expr<bool, EffectExprSchema> + Send + Sync + 'static,
    ) -> Self {
        self.def.activate_conditions.push(Box::new(condition));
        self
    }

    pub fn add_effect_trigger<E: EntityEvent, M>(
        mut self,
        observer: impl IntoObserverSystem<E, M> + Clone + Sync + 'static,
    ) -> Self {
        self.def
            .on_effect_triggers
            .push(effect_trigger(|effect| Some(effect.id()), observer));
        self
    }

    pub fn add_actor_trigger<E: EntityEvent, M>(
        mut self,
        observer: impl IntoObserverSystem<E, M> + Clone + Sync + 'static,
    ) -> Self {
        self.def.on_actor_triggers.push(effect_trigger(
            |effect| effect.get::<EffectTarget>().map(|target| target.0),
            observer,
        ));
        self
    }

    pub fn when_source_attribute<T: Attribute>(
        mut self,
        range: impl RangeBounds<T::Property> + Send + Sync + 'static,
    ) -> Self {
        let expr = IsAttributeWithinBounds::<T>::new(range, EffectSubject::Source);
        self.def.activate_conditions.push(Box::new(expr));
        self
    }

    pub fn when_target_attribute<T: Attribute>(
        mut self,
        range: impl RangeBounds<T::Property> + Send + Sync + 'static,
    ) -> Self {
        let expr = IsAttributeWithinBounds::<T>::new(range, EffectSubject::Target);
        self.def.activate_conditions.push(Box::new(expr));
        self
    }

    pub fn name(mut self, name: String) -> Self {
        self.def.effect_fn.push(Box::new(
            move |effect_entity: &mut EntityCommands, _: Entity| {
                effect_entity.insert(Name::new(name.clone()));
            },
        ));
        self
    }

    pub fn insert(mut self, bundle: impl Bundle + Copy) -> Self {
        self.def.effect_fn.push(Box::new(
            move |effect_entity: &mut EntityCommands, _: Entity| {
                effect_entity.insert(bundle);
            },
        ));
        self
    }

    /// Tags the effect, and grants the tag to the target while the effect exists.
    pub fn with_tag<T: Component + Default>(mut self) -> Self {
        self.def.granted_tags.push(TagSpec::of::<T>());
        self
    }

    pub fn requires_tag<T: Component + Default>(mut self) -> Self {
        self.def.required_tags.push(TagSpec::of::<T>());
        self
    }

    /// A target carrying `T` cannot receive this effect, and one the tag takes off mid-run stays off: revoking `T` does not bring it back.
    pub fn blocked_by_tag<T: Component + Default>(mut self) -> Self {
        self.def.blocked_by_tags.push(TagSpec::of::<T>());
        self
    }

    /// Removes the target's effects tagged `T` when this effect applies.
    pub fn cleanses<T: Component + Default>(mut self) -> Self {
        self.def.cleanses.push(TagSpec::of::<T>());
        self
    }

    pub fn with_stacking_policy(mut self, policy: EffectStackingPolicy) -> Self {
        self.def.stacking_policy = policy;
        self
    }

    pub fn build(self) -> EffectDef {
        self.def
    }
}
