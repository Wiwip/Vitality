use crate::ability::AbilityCooldown;
use crate::ability::systems::ActivateAbility;
use crate::ability::tasks::{AbilityTask, BeginTask, CancelTask, EndTask};
use crate::assets::AbilityDef;
use crate::attributes::Attribute;
use crate::context::{AbilityExprSchema, EffectExprSchema};
use crate::inspector::pretty_type_name;
use crate::modifier::{AttributeCalculatorCached, EffectSubject};
use crate::mutator::EntityActions;
use bevy::ecs::system::{IntoObserverSystem, StaticSystemParam};
use bevy::prelude::*;
use express_it::expr::Expr;
use express_it::frame::LazyPlan;
use express_it::logic::{BoolExpr, CompareExpr};
use num_traits::{AsPrimitive, Num};

pub struct AbilityBuilder {
    name: String,
    mutators: Vec<EntityActions>,
    triggers: Vec<EntityActions>,
    cost_condition: Vec<BoolExpr<AbilityExprSchema>>,
    cost_modifiers: LazyPlan,
    on_execute: Vec<LazyPlan>,
    scene: Box<dyn Fn() -> Box<dyn Scene> + Send + Sync>,
}

impl AbilityBuilder {
    pub fn new() -> AbilityBuilder {
        Self {
            name: "Ability".to_string(),
            mutators: Default::default(),
            triggers: vec![],
            cost_condition: vec![],
            cost_modifiers: LazyPlan::new(),
            on_execute: vec![],
            scene: Box::new(|| Box::new(())),
        }
    }

    pub fn with<T: Attribute>(
        mut self,
        value: impl Num + AsPrimitive<T::Property> + Copy + Send + Sync + 'static,
    ) -> AbilityBuilder {
        self.mutators.push(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.insert((T::new(value), AttributeCalculatorCached::<T>::default()));
            },
        ));
        self
    }

    pub fn with_cost<T: Attribute>(
        mut self,
        cost: impl Into<Expr<T::Property, AbilityExprSchema>>,
    ) -> Self
    where
        Expr<T::Property, AbilityExprSchema>: CompareExpr<AbilityExprSchema>,
    {
        let cost_expr = cost.into();
        let cost_assignment = T::sub(EffectSubject::Source, cost_expr.clone());
        self.cost_modifiers = self.cost_modifiers.step(cost_assignment);

        let cost_expr = cost_expr.le(T::src());
        self.cost_condition.push(cost_expr);
        self
    }

    pub fn with_cooldown(mut self, expr: impl Into<Expr<f64, EffectExprSchema>>) -> Self {
        let val = expr.into();

        self.mutators.push(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.try_insert(AbilityCooldown {
                    timer: Timer::from_seconds(0.0, TimerMode::Once),
                    value: val.clone(),
                });
            },
        ));
        self
    }

    pub fn add_execution<E: EntityEvent, B: Bundle, M>(
        mut self,
        observer: impl IntoObserverSystem<E, B, M> + Clone + Send + Sync + 'static,
    ) -> Self {
        self.mutators.push(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.observe(observer.clone());
            },
        ));
        self
    }

    pub fn on_execute(mut self, plan: LazyPlan) -> Self {
        self.on_execute.push(plan);
        self
    }

    pub fn add_trigger<E: EntityEvent, B: Bundle, M>(
        mut self,
        observer: impl IntoObserverSystem<E, B, M> + Clone + Send + Sync + 'static,
    ) -> Self {
        self.triggers.push(EntityActions::new(
            move |actor_commands: &mut EntityCommands| {
                let mut observer = Observer::new(observer.clone());
                observer.watch_entity(actor_commands.id());

                actor_commands.commands().spawn((
                    observer,
                    Name::new(format!("On<{}>", pretty_type_name::<E>())),
                ));
            },
        ));
        self
    }

    pub fn with_tag<T: Component + Default>(mut self) -> Self {
        self.mutators.push(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.try_insert(T::default());
            },
        ));
        self
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    /*pub fn add_task<T: AbilityTask>(mut self) -> Self {
        self.mutators.push(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.observe(
                    |trigger: On<BeginTask>,
                     mut query: Query<T::Query>,
                     params: StaticSystemParam<T::Param>| {
                        let item = query.get_mut(trigger.event_target()).unwrap();
                        let mut param_items = params.into_inner();
                        T::on_begin(item, &mut param_items);
                    },
                );
                entity_commands.observe(|trigger: On<CancelTask>, mut query: Query<T::Query>| {
                    let item = query.get_mut(trigger.event_target()).unwrap();
                    T::on_cancel(item);
                });
                entity_commands.observe(|trigger: On<EndTask>, mut query: Query<T::Query>| {
                    let item = query.get_mut(trigger.event_target()).unwrap();
                    T::on_end(item);
                });
            },
        ));
        self
    }*/

    pub fn apply_scene<S, F>(mut self, scene_factory: F) -> Self
    where
        S: Scene + 'static,
        F: Fn() -> S + Send + Sync + 'static,
    {
        self.scene = Box::new(move || Box::new(scene_factory()));
        self
    }

    pub fn build(self) -> AbilityDef {
        AbilityDef {
            name: self.name,
            description: "".to_string(),
            mutators: self.mutators,
            observers: self.triggers,
            cost_condition: self.cost_condition,
            execution_conditions: vec![],
            cost_modifiers: self.cost_modifiers,

            scene: self.scene,
            on_execute: self.on_execute,
        }
    }
}
