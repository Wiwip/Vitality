use crate::context::{AbilityExprContext, AbilityExprContextMut, AbilityExprSchema};
use crate::definition::AbilityDef;
use bevy::ecs::system::IntoObserverSystem;
use bevy::prelude::*;
use express_it::expr::{AsExpression, BoolExpr, StoredExpr};
use express_it::logic::ExprCmpLe;
use express_it::nodes::LiteralNode;
use express_it::plan::{AssignmentStep, Plan};
use num_traits::{AsPrimitive, Num};
use vitality_core::attributes::Attribute;
use vitality_core::context::ActorExprSchema;
use vitality_core::modifier::AttributeCalculatorCached;
use vitality_core::mutator::EntityActions;
use vitality_core::tags::TagSpec;
use vitality_core::util::pretty_type_name;

pub struct AbilityBuilder {
    name: String,
    mutators: Vec<EntityActions>,
    triggers: Vec<EntityActions>,
    cost_condition: Vec<StoredExpr<bool, AbilityExprSchema>>,
    execution_condition: Vec<StoredExpr<bool, ActorExprSchema>>,
    cooldown: StoredExpr<f32, AbilityExprSchema>,
    cost_modifiers: Plan<AbilityExprSchema>,
    on_execute: Vec<Plan<AbilityExprSchema>>,
    recovery_condition: Vec<BoolExpr<ActorExprSchema>>,
    scene: Box<dyn Fn() -> Box<dyn Scene> + Send + Sync>,
    tags: Vec<TagSpec>,
    activation_owned_tags: Vec<TagSpec>,
    activation_requires: Vec<TagSpec>,
    activation_blocked_by: Vec<TagSpec>,
    blocks_abilities_with: Vec<TagSpec>,
    cancels_abilities_with: Vec<TagSpec>,
}

impl Default for AbilityBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AbilityBuilder {
    pub fn new() -> AbilityBuilder {
        Self {
            name: "Ability".to_string(),
            mutators: Default::default(),
            triggers: vec![],
            cost_condition: vec![],
            execution_condition: vec![],
            cooldown: Box::new(LiteralNode::<f32> { value: 0.0 }),
            cost_modifiers: Plan::new(),
            on_execute: vec![],
            recovery_condition: vec![],
            scene: Box::new(|| Box::new(())),
            tags: vec![],
            activation_owned_tags: vec![],
            activation_requires: vec![],
            activation_blocked_by: vec![],
            blocks_abilities_with: vec![],
            cancels_abilities_with: vec![],
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
        let node_expr = express_it::nodes::Node {
            expr: cost.as_expr(),
            _marker: Default::default(),
        };

        let get_attr = T::src();
        let final_expr = get_attr - node_expr;

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
            expr: final_expr,
            cache_key: None,
            _marker: std::marker::PhantomData,
        };

        // every cost is paid, not just the last one declared
        let plan = std::mem::replace(&mut self.cost_modifiers, Plan::new());
        self.cost_modifiers = plan.step(step);

        let t_src = express_it::nodes::Node::<T::Property, AbilityExprSchema, _>::new(
            |ctx: &AbilityExprContext| {
                // a caster without the attribute can't pay
                ctx.caster_ref
                    .get::<T>()
                    .map_or(T::Property::default(), |attribute| attribute.current_value())
            },
        );

        // This will now compile because E satisfies the comparison trait and hasn't been moved
        let cost_condition = node_expr.le(t_src);

        self.cost_condition.push(Box::new(cost_condition));
        self
    }

    pub fn with_activation_condition(mut self, expr: StoredExpr<bool, ActorExprSchema>) -> Self {
        self.execution_condition.push(expr);
        self
    }

    pub fn with_cooldown<E: AsExpression<f32, AbilityExprSchema, Target: Copy + 'static>>(
        mut self,
        cost: E,
    ) -> Self
    where
        <E as AsExpression<f32, AbilityExprSchema>>::Target:
            express_it::expr::Expr<f32, AbilityExprSchema>,
    {
        let node_expr = express_it::nodes::Node {
            expr: cost.as_expr(),
            _marker: Default::default(),
        };
        self.cooldown = Box::new(node_expr);
        self
    }

    pub fn add_execution<E: EntityEvent, M>(
        mut self,
        observer: impl IntoObserverSystem<E, M> + Clone + Sync + 'static,
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

    pub fn add_trigger<E: EntityEvent, M>(
        mut self,
        observer: impl IntoObserverSystem<E, M> + Clone + Sync + 'static,
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
        self.tags.push(TagSpec::of::<T>());
        self
    }

    /// The caster carries `T` while this ability is active.
    pub fn activation_owned_tag<T: Component + Default>(mut self) -> Self {
        self.activation_owned_tags.push(TagSpec::of::<T>());
        self
    }

    pub fn activation_requires<T: Component + Default>(mut self) -> Self {
        self.activation_requires.push(TagSpec::of::<T>());
        self
    }

    /// The caster can't activate this while carrying `T`, e.g. a stun.
    pub fn activation_blocked_by<T: Component + Default>(mut self) -> Self {
        self.activation_blocked_by.push(TagSpec::of::<T>());
        self
    }

    pub fn blocks_abilities_with<T: Component + Default>(mut self) -> Self {
        self.blocks_abilities_with.push(TagSpec::of::<T>());
        self
    }

    pub fn cancels_abilities_with<T: Component + Default>(mut self) -> Self {
        self.cancels_abilities_with.push(TagSpec::of::<T>());
        self
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    pub fn set_tasks<S, F>(mut self, scene_factory: F) -> Self
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
            execution_conditions: self.execution_condition,
            cost_modifiers: self.cost_modifiers,

            cooldown: self.cooldown,
            task_scene: self.scene,
            recovery_condition: self.recovery_condition,
            on_execute: self.on_execute,
            tags: self.tags,
            activation_owned_tags: self.activation_owned_tags,
            activation_requires: self.activation_requires,
            activation_blocked_by: self.activation_blocked_by,
            blocks_abilities_with: self.blocks_abilities_with,
            cancels_abilities_with: self.cancels_abilities_with,
        }
    }
}
