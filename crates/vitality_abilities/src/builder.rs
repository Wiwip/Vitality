use crate::AbilityOf;
use crate::context::{AbilityExprContext, AbilityExprContextMut, AbilityExprSchema};
use crate::definition::AbilityDef;
use bevy_ecs::prelude::*;
use bevy_ecs::system::IntoObserverSystem;
use bevy_log::{error, warn};
use bevy_scene::Scene;
use express_it::expr::{AsExpression, StoredExpr};
use express_it::logic::ExprCmpLe;
use express_it::nodes::LiteralNode;
use express_it::plan::{AssignmentStep, Plan};
use vitality_core::attributes::Attribute;
use vitality_core::context::ActorExprSchema;
use vitality_core::mutator::EntityActions;
use vitality_core::tags::TagSpec;
use vitality_core::util::pretty_type_name;
use vitality_core::value::AttributeValue;

pub struct AbilityBuilder {
    name: String,
    mutators: Vec<EntityActions>,
    triggers: Vec<EntityActions>,
    cost_condition: Vec<StoredExpr<bool, AbilityExprSchema>>,
    execution_condition: Vec<StoredExpr<bool, ActorExprSchema>>,
    cooldown: StoredExpr<f32, AbilityExprSchema>,
    cost_modifiers: Plan<AbilityExprSchema>,
    on_execute: Vec<Plan<AbilityExprSchema>>,
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
            scene: Box::new(|| Box::new(())),
            tags: vec![],
            activation_owned_tags: vec![],
            activation_requires: vec![],
            activation_blocked_by: vec![],
            blocks_abilities_with: vec![],
            cancels_abilities_with: vec![],
        }
    }

    pub fn with<T: Attribute>(mut self, value: T::Property) -> AbilityBuilder {
        self.mutators.push(EntityActions::new(
            move |entity_commands: &mut EntityCommands| {
                entity_commands.insert(T::new(value));
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

        // paid from the base the setter writes, never current, which would spend the modifiers on top
        let step = AssignmentStep {
            setter_fn: |ctx: &mut AbilityExprContextMut, cost: T::Property| match ctx
                .caster_mut
                .get_mut::<T>()
            {
                None => {
                    error!(
                        "{} not found on the caster: the cost is not paid, put the component on \
                         the caster",
                        pretty_type_name::<T>()
                    )
                }
                Some(mut attr) => {
                    let base = attr.base();
                    // stale current_value: a sibling may have drained the pool, so pay in full or not at all
                    if base < cost {
                        warn!(
                            "{} cannot pay its cost: base {base} no longer covers {cost}, the cost \
                             is not paid",
                            pretty_type_name::<T>()
                        );
                        return;
                    }
                    attr.set_base_value(<T::Property as AttributeValue>::sub(base, cost))
                }
            },
            expr: node_expr,
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
                    .map_or(T::Property::default(), |attribute| {
                        attribute.current_value()
                    })
            },
        );

        let cost_condition = node_expr.le(t_src);

        self.cost_condition.push(Box::new(cost_condition));
        self
    }

    /// evaluated against the caster only: this schema's bool condition is `HasComponent`, `Source` only
    pub fn with_activation_condition(mut self, expr: StoredExpr<bool, ActorExprSchema>) -> Self {
        self.execution_condition.push(expr);
        self
    }

    pub fn with_cooldown<E: AsExpression<f32, AbilityExprSchema, Target: Copy + 'static>>(
        mut self,
        cooldown: E,
    ) -> Self
    where
        <E as AsExpression<f32, AbilityExprSchema>>::Target:
            express_it::expr::Expr<f32, AbilityExprSchema>,
    {
        let node_expr = express_it::nodes::Node {
            expr: cooldown.as_expr(),
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
            move |ability_commands: &mut EntityCommands| {
                let observer = observer.clone();
                ability_commands.queue(move |mut ability: EntityWorldMut| {
                    let ability_id = ability.id();
                    let Some(caster) = ability.get::<AbilityOf>().map(|owner| owner.0) else {
                        warn!("[{ability_id}] Ability has no caster: its trigger watches nothing.");
                        return;
                    };
                    let mut observer = Observer::new(observer);
                    observer.watch_entity(caster);

                    // it watches the caster but belongs to the ability, so revoke takes it along
                    ability.world_scope(|world| {
                        world.spawn((
                            ChildOf(ability_id),
                            observer,
                            Name::new(format!("On<{}>", pretty_type_name::<E>())),
                        ));
                    });
                });
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

    /// the caster must carry `T`; a miss is `Blocked`, not `ConditionFailed`, as it runs first
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
            mutators: self.mutators,
            observers: self.triggers,
            cost_condition: self.cost_condition,
            execution_conditions: self.execution_condition,
            cost_modifiers: self.cost_modifiers,

            cooldown: self.cooldown,
            task_scene: self.scene,
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
