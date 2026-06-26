use crate::assets::AbilityDef;
use crate::attributes::Attribute;
use crate::context::{
    AbilityExprContext, AbilityExprContextMut, AbilityExprSchema, ActorExprSchema, EffectExprSchema,
};
use crate::inspector::pretty_type_name;
use crate::modifier::AttributeCalculatorCached;
use crate::mutator::EntityActions;
use bevy::ecs::system::IntoObserverSystem;
use bevy::prelude::*;
use express_it::expr::{AsExpression, BoolExpr, Expr, StoredExpr};
use express_it::logic::ExprCmpLe;
use express_it::plan::{AssignmentStep, Plan};
use num_traits::{AsPrimitive, Num};
use crate::ability::AbilityCooldown;

pub struct AbilityBuilder {
    name: String,
    mutators: Vec<EntityActions>,
    triggers: Vec<EntityActions>,
    cost_condition: Vec<StoredExpr<bool, AbilityExprSchema>>,
    cost_modifiers: Plan<AbilityExprSchema>,
    on_execute: Vec<Plan<AbilityExprSchema>>,
    recovery_condition: Vec<BoolExpr<ActorExprSchema>>,
    scene: Box<dyn Fn() -> Box<dyn Scene> + Send + Sync>,
}

impl AbilityBuilder {
    pub fn new() -> AbilityBuilder {
        Self {
            name: "Ability".to_string(),
            mutators: Default::default(),
            triggers: vec![],
            cost_condition: vec![],
            cost_modifiers: Plan::new(),
            on_execute: vec![],
            recovery_condition: vec![],
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
        cost: impl AsExpression<T::Property, AbilityExprSchema, Target: Copy + 'static>,
    ) -> Self
    where
        T::Property: std::cmp::PartialOrd + Copy + 'static,
    {
        let cost_expr = cost.as_expr();
        let node_expr = express_it::nodes::Node {
            expr: cost_expr,
            _marker: Default::default(),
        };
        let step = AssignmentStep {
            setter_fn: |ctx: &mut AbilityExprContextMut, val: T::Property| match ctx
                .caster_mut
                .get_mut::<T>()
            {
                None => {
                    error!("Error during assignment step. No attribute found.")
                }
                Some(mut attr) => attr.set_base_value(val),
            },
            expr: cost_expr,
            cache_key: None,
            _marker: std::marker::PhantomData,
        };

        let plan = Plan::new().step(step);
        self.cost_modifiers = plan;

        let t_src = express_it::nodes::Node::<T::Property, AbilityExprSchema, _>::new(
            |ctx: &AbilityExprContext| ctx.caster_ref.get::<T>().unwrap().current_value(),
        );

        // This will now compile because E satisfies the comparison trait and hasn't been moved
        let cost_condition = node_expr.le(t_src);

        self.cost_condition.push(Box::new(cost_condition));
        self
    }

    pub fn with_cooldown(mut self, value: f32) -> Self {
        self.mutators.push(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.try_insert(AbilityCooldown::new(value));
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

    pub fn on_execute(mut self, plan: Plan<AbilityExprSchema>) -> Self {
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

            task_scene: self.scene,
            recovery_condition: self.recovery_condition,
            on_execute: self.on_execute,
        }
    }
}
