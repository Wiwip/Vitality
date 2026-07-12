use crate::ability::Ability;
use crate::context::{ActorProvider, ActorProviderMut, Source, Target};
use crate::effect::{AttributeDependents, Effect};
use crate::inspector::pretty_type_name;
use crate::math::{AbsDiff, SaturatingAttributes};
use crate::modifier::{AttributeCalculator, AttributeCalculatorCached};
use crate::systems::MarkNodeDirty;
use crate::{AttributesMut, AttributesRef};
use bevy::ecs::component::Mutable;
use bevy::ecs::query::QueryData;
use bevy::log::error;
use bevy::prelude::{Changed, Commands, Component, Entity, EntityEvent, Insert, On, Query, Reflect, RelationshipTarget, TypePath};
use bevy::reflect::{GetTypeRegistration, Typed, reflect_trait};
use express_it::expr::{AsExpression, Context, ContextMut, Expr};
use express_it::nodes::Node;
use express_it::plan::AssignmentStep;
use num_traits::NumCast;
pub use num_traits::{
    AsPrimitive, Bounded, FromPrimitive, Num, NumAssign, NumAssignOps, NumOps, Saturating,
    SaturatingAdd, SaturatingMul, Zero,
};
use std::any::TypeId;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::iter::Sum;
use std::marker::PhantomData;

pub trait GameValue
where
    Self: Num + NumOps + NumAssign + NumAssignOps + NumCast,
    Self: Default + PartialOrd + Copy + Debug + Display,
    Self: GetTypeRegistration + Typed + Send + Sync,
    Self: SaturatingAttributes<Output = Self> + Sum + Bounded + AbsDiff,
    Self: FromPrimitive + AsPrimitive<f64> + Reflect,
{
}

impl<T> GameValue for T
where
    Self: Num + NumOps + NumAssign + NumAssignOps + NumCast,
    Self: Default + PartialOrd + Copy + Debug + Display,
    Self: GetTypeRegistration + Typed + Send + Sync,
    Self: SaturatingAttributes<Output = Self> + Sum + Bounded + AbsDiff,
    Self: FromPrimitive + AsPrimitive<f64> + Reflect,
{
}

pub trait Attribute
where
    Self: Component<Mutability = Mutable> + Copy + Debug + Display,
    Self: Reflect + TypePath + GetTypeRegistration,
{
    type Property: GameValue;

    fn new<T: Num + AsPrimitive<Self::Property> + Copy>(value: T) -> Self;
    fn base_value(&self) -> Self::Property;
    fn base(&self) -> Self::Property;
    fn set_base_value(&mut self, value: Self::Property);
    fn current_value(&self) -> Self::Property;
    fn val(&self) -> Self::Property;
    fn set_current_value(&mut self, value: Self::Property);

    // Helper to wrap attribute access in an Expression
    fn src<C>() -> Node<Self::Property, C, AttributeVar<Self, Self::Property, C>>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>:
            ActorProvider<'a, 'b, Source, ActorRef = AttributesRef<'a, 'b>>,
    {
        Self::at::<Source, C>()
    }
    fn dst<C>() -> Node<Self::Property, C, AttributeVar<Self, Self::Property, C>>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>:
            ActorProvider<'a, 'b, Target, ActorRef = AttributesRef<'a, 'b>>,
    {
        Self::at::<Target, C>()
    }
    fn ability<C>() -> Node<Self::Property, C, AttributeVar<Self, Self::Property, C>>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>:
            ActorProvider<'a, 'b, Ability, ActorRef = AttributesRef<'a, 'b>>,
    {
        Self::at::<Ability, C>()
    }
    fn effect<C>() -> Node<Self::Property, C, AttributeVar<Self, Self::Property, C>>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>:
            ActorProvider<'a, 'b, Effect, ActorRef = AttributesRef<'a, 'b>>,
    {
        Self::at::<Effect, C>()
    }
    fn at<Role, C>() -> Node<Self::Property, C, AttributeVar<Self, Self::Property, C>>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>:
            ActorProvider<'a, 'b, Role, ActorRef = AttributesRef<'a, 'b>>,
    {
        Node {
            expr: AttributeVar {
                fetch_fn: |ctx: &C::ContextItem<'_, '_>| {
                    ctx.get_actor()
                        .get::<Self>()
                        .expect(&format!("{} not found on {}", pretty_type_name::<Self>(), pretty_type_name::<Role>()))
                        .current_value()
                },
                _marker: Default::default(),
            },
            _marker: Default::default(),
        }
    }
    fn at_base<Role, C>() -> Node<Self::Property, C, AttributeVar<Self, Self::Property, C>>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>:
            ActorProvider<'a, 'b, Role, ActorRef = AttributesRef<'a, 'b>>,
    {
        Node {
            expr: AttributeVar {
                fetch_fn: |ctx: &C::ContextItem<'_, '_>| {
                    ctx.get_actor().get::<Self>().unwrap().base_value()
                },
                _marker: Default::default(),
            },
            _marker: Default::default(),
        }
    }

    fn add<Role, C: ContextMut>(
        expr: impl AsExpression<Self::Property, C, Target: Copy + Send + Sync + 'static>,
    ) -> AssignmentStep<
        Self::Property,
        impl Expr<Self::Property, C> + Copy + Send + Sync + 'static,
        impl Fn(&mut C::ContextItemMut<'_, '_>, Self::Property) + 'static + Send + Sync,
    >
    where
        Role: 'static,
        C: ContextMut + 'static,
        for<'a, 'b> C::ContextItemMut<'a, 'b>:
            ActorProviderMut<'a, 'b, Role, ActorMut = AttributesMut<'a, 'b>>,
    {
        let expr = expr.as_expr();

        AssignmentStep {
            setter_fn: |ctx: &mut C::ContextItemMut<'_, '_>, expr_val: Self::Property| {
                let actor = ActorProviderMut::<Role>::get_actor_mut(ctx);
                match actor.get_mut::<Self>() {
                    None => {
                        error!("Error during assignment step. No attribute found.")
                    }
                    Some(mut attr) => {
                        let base = attr.base();
                        attr.set_base_value(base + expr_val)
                    }
                }
            },
            expr,
            cache_key: None,
            _marker: Default::default(),
        }
    }

    fn sub<Role, C: ContextMut>(
        expr: impl AsExpression<Self::Property, C, Target: Copy + Send + Sync + 'static>,
    ) -> AssignmentStep<
        Self::Property,
        impl Expr<Self::Property, C> + Copy + Send + Sync + 'static,
        impl Fn(&mut C::ContextItemMut<'_, '_>, Self::Property) + 'static + Send + Sync,
    >
    where
        Role: 'static,
        C: ContextMut + 'static,
        for<'a, 'b> C::ContextItemMut<'a, 'b>:
            ActorProviderMut<'a, 'b, Role, ActorMut = AttributesMut<'a, 'b>>,
    {
        let expr = expr.as_expr();

        AssignmentStep {
            setter_fn: |ctx: &mut C::ContextItemMut<'_, '_>, expr_val: Self::Property| {
                let actor = ActorProviderMut::<Role>::get_actor_mut(ctx);
                match actor.get_mut::<Self>() {
                    None => {
                        error!("Error during assignment step. No attribute found.")
                    }
                    Some(mut attr) => {
                        let base = attr.base();
                        attr.set_base_value(base - expr_val)
                    }
                }
            },
            expr,
            cache_key: None,
            _marker: Default::default(),
        }
    }

    fn set<Role, C: ContextMut>(
        expr: impl AsExpression<Self::Property, C, Target: Copy + Send + Sync + 'static>,
    ) -> AssignmentStep<
        Self::Property,
        impl Expr<Self::Property, C> + Copy + Send + Sync + 'static,
        impl Fn(&mut C::ContextItemMut<'_, '_>, Self::Property) + 'static + Send + Sync,
    >
    where
        Role: 'static,
        C: ContextMut + 'static,
        for<'a, 'b> C::ContextItemMut<'a, 'b>:
            ActorProviderMut<'a, 'b, Role, ActorMut = AttributesMut<'a, 'b>>,
    {
        let expr = expr.as_expr();

        AssignmentStep {
            setter_fn: |ctx: &mut C::ContextItemMut<'_, '_>, expr_val: Self::Property| {
                let actor = ActorProviderMut::<Role>::get_actor_mut(ctx);
                match actor.get_mut::<Self>() {
                    None => {
                        error!("Error during assignment step. No attribute found.")
                    }
                    Some(mut attr) => attr.set_base_value(expr_val),
                }
            },
            expr,
            cache_key: None,
            _marker: Default::default(),
        }
    }
}

#[macro_export]
macro_rules! attribute_impl {
    ( $StructName:ident, $ValueType:ty ) => {
        #[derive(
            bevy::prelude::Component,
            Clone,
            Copy,
            bevy::prelude::Reflect,
            Debug,
            bevy::prelude::FromTemplate,
            bevy::prelude::Deref,
        )]
        #[require($crate::modifier::AttributeCalculatorCached<$StructName>)]
        #[reflect(Component, AccessAttribute)]
        pub struct $StructName {
            pub base_value: $ValueType,
            #[deref]
            current_value: $ValueType,
        }

        impl $crate::attributes::Attribute for $StructName {
            type Property = $ValueType;

            fn new<T>(value: T) -> Self
            where
                T: $crate::num_traits::Num + $crate::num_traits::AsPrimitive<Self::Property> + Copy,
            {
                Self {
                    base_value: value.as_(),
                    current_value: value.as_(),
                }
            }
            #[inline(always)]
            fn base_value(&self) -> $ValueType {
                self.base_value
            }
            #[inline(always)]
            fn base(&self) -> $ValueType {
                self.base_value
            }
            fn set_base_value(&mut self, value: $ValueType) {
                self.base_value = value;
            }
            #[inline(always)]
            fn current_value(&self) -> $ValueType {
                self.current_value
            }
            #[inline(always)]
            fn val(&self) -> $ValueType {
                self.current_value
            }
            fn set_current_value(&mut self, value: $ValueType) {
                self.current_value = value;
            }
        }

        impl std::fmt::Display for $StructName {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}: {}", stringify!($StructName), self.current_value)
            }
        }
    };
}

#[macro_export]
macro_rules! attribute {
    ( $StructName:ident ) => {
        $crate::attribute_impl!($StructName, f32);
    };
    ( $StructName:ident, $ValueType:ty  ) => {
        $crate::attribute_impl!($StructName, $ValueType);
    };
}

#[macro_export]
macro_rules! tag {
    ( $StructName:ident ) => {
        #[derive(bevy::prelude::Component, bevy::prelude::Reflect, Default, Copy, Clone, Debug, bevy::prelude::FromTemplate)]
        #[reflect(Component)]
        pub struct $StructName;
    };
}

#[derive(Component, Default, Debug, Copy, Clone)]
pub struct ManageAttributes;

#[derive(QueryData, Debug)]
#[query_data(mutable, derive(Debug))]
pub struct AttributeQueryData<T: Attribute + 'static> {
    pub entity: Entity,
    pub attribute: &'static mut T,
    pub calculator_cache: &'static mut AttributeCalculatorCached<T>,
}

impl<T: Attribute> AttributeQueryDataItem<'_, '_, T> {
    pub fn update_attribute(&mut self, calculator: &AttributeCalculator<T>) -> bool {
        let old_val = self.attribute.current_value();
        let new_val = calculator.eval(self.attribute.base_value());

        let has_changed = old_val.are_different(new_val);
        if has_changed {
            self.attribute.set_current_value(new_val);
        }
        has_changed
    }

    pub fn update_attribute_from_cache(&mut self) -> bool {
        let old_val = self.attribute.current_value();
        let new_val = self
            .calculator_cache
            .calculator
            .eval(self.attribute.base_value());

        let has_changed = old_val.are_different(new_val);
        if has_changed {
            self.attribute.set_current_value(new_val);
        }
        has_changed
    }
}

#[reflect_trait] // Generates a `ReflectMyTrait` type
pub trait AccessAttribute {
    fn access_base_value(&self) -> f64;
    fn access_current_value(&self) -> f64;
    fn name(&self) -> String;
}

impl<T> AccessAttribute for T
where
    T: Attribute,
{
    fn access_base_value(&self) -> f64 {
        self.base_value().as_()
    }
    fn access_current_value(&self) -> f64 {
        self.current_value().as_()
    }
    fn name(&self) -> String {
        pretty_type_name::<T>()
    }
}

pub fn on_add_attribute<T: Attribute>(trigger: On<Insert, T>, mut commands: Commands) {
    commands.trigger(MarkNodeDirty::<T> {
        entity: trigger.event_target(),
        phantom_data: Default::default(),
    });
}

#[derive(EntityEvent)]
pub struct AttributeDependencyChanged<T> {
    pub entity: Entity,
    phantom_data: PhantomData<T>,
}

pub fn on_change_notify_attribute_dependencies<T: Attribute>(
    query: Query<&AttributeDependents<T>, Changed<T>>,
    mut commands: Commands,
) {
    for dependents in query.iter() {
        let unique_entities: HashSet<Entity> = dependents.iter().collect();
        let notify_targets: Vec<Entity> = unique_entities.into_iter().collect();

        notify_targets.iter().for_each(|target| {
            commands.trigger(AttributeDependencyChanged::<T> {
                entity: *target,
                phantom_data: Default::default(),
            });
        });
    }
}

pub fn on_change_notify_attribute_parents<T: Attribute>(
    query: Query<Entity, Changed<T>>,
    mut commands: Commands,
) {
    for entity in query.iter() {
        commands.trigger(MarkNodeDirty::<T> {
            entity,
            phantom_data: Default::default(),
        });
    }
}

pub struct AttributeVar<T, N, C: Context> {
    pub fetch_fn: for<'w, 's> fn(&C::ContextItem<'w, 's>) -> N,
    _marker: PhantomData<T>,
}
impl<T, N, C: Context> Clone for AttributeVar<T, N, C> {
    fn clone(&self) -> Self {
        Self {
            fetch_fn: self.fetch_fn,
            _marker: Default::default(),
        }
    }
}
impl<T: 'static, N: 'static, C: Context + 'static> Expr<N, C> for AttributeVar<T, N, C> {
    #[inline(always)]
    fn eval(&self, ctx: &C::ContextItem<'_, '_>) -> N {
        (self.fetch_fn)(ctx)
    }

    fn get_dependencies(&self, deps: &mut HashSet<TypeId>) {
        deps.insert(TypeId::of::<T>());
    }
}
impl<T, N, C: Context> Copy for AttributeVar<T, N, C> {}
impl<T, N: fmt::Display, C: Context> fmt::Display for AttributeVar<T, N, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", pretty_type_name::<T>())
    }
}
