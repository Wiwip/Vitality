use crate::context::{AbilityExprSchema, ActorExprSchema, EffectExprSchema};
use crate::effect::AttributeDependents;
use crate::inspector::pretty_type_name;
use crate::math::{AbsDiff, SaturatingAttributes};
use crate::modifier::{AttributeCalculator, AttributeCalculatorCached};
use crate::systems::MarkNodeDirty;
use bevy::ecs::component::Mutable;
use bevy::ecs::query::QueryData;
use bevy::prelude::*;
use bevy::reflect::{GetTypeRegistration, Typed};
use express_it::expr::{Expr, ExprNode, ExprSchema, SelectExprNodeImpl};
use express_it::frame::Assignment;
use num_traits::NumCast;
pub use num_traits::{
    AsPrimitive, Bounded, FromPrimitive, Num, NumAssign, NumAssignOps, NumOps, Saturating,
    SaturatingAdd, SaturatingMul, Zero,
};
use smol_str::SmolStr;
use std::any::Any;
use std::collections::HashSet;
use std::fmt::Debug;
use std::fmt::Display;
use std::iter::Sum;
use std::marker::PhantomData;

pub trait Value
where
    Self: Num + NumOps + NumAssign + NumAssignOps + NumCast,
    Self: Default + PartialOrd + Copy + Debug + Display,
    Self: GetTypeRegistration + Typed + Send + Sync,
    Self: SaturatingAttributes<Output = Self> + Sum + Bounded + AbsDiff,
    Self: FromPrimitive + AsPrimitive<f64> + Reflect,
    Self: SelectExprNodeImpl<EffectExprSchema, Property = Self>,
    Self: SelectExprNodeImpl<ActorExprSchema, Property = Self>,
    Self: SelectExprNodeImpl<AbilityExprSchema, Property = Self>,
{
}

impl<T> Value for T
where
    Self: Num + NumOps + NumAssign + NumAssignOps + NumCast,
    Self: Default + PartialOrd + Copy + Debug + Display,
    Self: GetTypeRegistration + Typed + Send + Sync,
    Self: SaturatingAttributes<Output = Self> + Sum + Bounded + AbsDiff,
    Self: FromPrimitive + AsPrimitive<f64> + Reflect,
    Self: SelectExprNodeImpl<EffectExprSchema, Property = Self>,
    Self: SelectExprNodeImpl<ActorExprSchema, Property = Self>,
    Self: SelectExprNodeImpl<AbilityExprSchema, Property = Self>,
{
}

pub trait Attribute
where
    Self: Component<Mutability = Mutable> + Copy + Debug + Display,
    Self: Reflect + TypePath + GetTypeRegistration,
{
    type Property: Value;
    type ExprType<S: ExprSchema>: ExprNode<Self::Property, S>;

    fn new<T: Num + AsPrimitive<Self::Property> + Copy>(value: T) -> Self;
    fn base_value(&self) -> Self::Property;
    fn base(&self) -> Self::Property;
    fn set_base_value(&mut self, value: Self::Property);
    fn current_value(&self) -> Self::Property;
    fn val(&self) -> Self::Property;
    fn borrow_current_value(&self) -> &Self::Property;
    fn set_current_value(&mut self, value: Self::Property);
    // Helper to wrap attribute access in an Expression
    fn src<S: ExprSchema>() -> Expr<Self::Property, S>
    where
        Self::Property: SelectExprNodeImpl<S>;
    fn dst<S: ExprSchema>() -> Expr<Self::Property, S>
    where
        Self::Property: SelectExprNodeImpl<S>;
    fn parent<S: ExprSchema>() -> Expr<Self::Property, S>
    where
        Self::Property: SelectExprNodeImpl<S>;
    fn scoped<S: ExprSchema>(subject: impl Into<SmolStr>) -> Expr<Self::Property, S>
    where
        Self::Property: SelectExprNodeImpl<S>;
    fn lit<S: ExprSchema>(value: Self::Property) -> Expr<Self::Property, S>
    where
        Self::Property: SelectExprNodeImpl<S>;
    // Expression helpers
    fn set<S: ExprSchema>(
        subject: impl Into<SmolStr>,
        expr: impl Into<Expr<Self::Property, S>>,
    ) -> Assignment<Self::Property, S>
    where
        Self::Property: SelectExprNodeImpl<S>;
    fn add<S: ExprSchema>(
        subject: impl Into<SmolStr> + Copy,
        expr: impl Into<Expr<Self::Property, S>>,
    ) -> Assignment<Self::Property, S>
    where
        Self::Property: SelectExprNodeImpl<S>;
    fn sub<S: ExprSchema>(
        subject: impl Into<SmolStr> + Copy,
        expr: impl Into<Expr<Self::Property, S>>,
    ) -> Assignment<Self::Property, S>
    where
        Self::Property: SelectExprNodeImpl<S>;
}

// Move expression-related functions to this subtrait.
pub trait ExprAttribute: Attribute {}

#[macro_export]
macro_rules! attribute_impl {
    ( $StructName:ident, $ValueType:ty ) => {
        #[derive(bevy::prelude::Component, Clone, Copy, bevy::prelude::Reflect, Debug, bevy::prelude::FromTemplate, bevy::prelude::Deref)]
        #[require($crate::modifier::AttributeCalculatorCached<$StructName>)]
        #[reflect(Component, AccessAttribute)]
        pub struct $StructName {
            pub base_value: $ValueType,
            #[deref]
            current_value: $ValueType,
        }

        impl $crate::attributes::Attribute for $StructName {
            type Property = $ValueType;
            type ExprType<S: ExprSchema> = $crate::express_it::expr::SelectExprNode<$ValueType, S>;

            fn new<T>(value: T) -> Self
            where
                T: $crate::num_traits::Num + $crate::num_traits::AsPrimitive<Self::Property> + Copy,
            {
                Self {
                    base_value: value.as_(),
                    current_value: value.as_(),
                }
            }
            fn base_value(&self) -> $ValueType {
                self.base_value
            }
            fn base(&self) -> $ValueType {
                self.base_value
            }
            fn set_base_value(&mut self, value: $ValueType) {
                self.base_value = value;
            }
            fn current_value(&self) -> $ValueType {
                self.current_value
            }
            fn val(&self) -> $ValueType {
                self.current_value
            }
            fn borrow_current_value(&self) -> &$ValueType {
                &self.current_value
            }
            fn set_current_value(&mut self, value: $ValueType) {
                self.current_value = value;
            }
            fn src<S: ExprSchema>() -> $crate::express_it::expr::Expr<Self::Property, S> {
                $crate::express_it::expr::Expr::new(std::sync::Arc::new(Self::ExprType::Attribute(
                    $crate::express_it::context::Path::from_type_name::<Self>(
                        $crate::modifier::EffectSubject::Source,
                        "current_value",
                    ),
                )))
            }
            fn dst<S: ExprSchema>() -> $crate::express_it::expr::Expr<Self::Property, S> {
                $crate::express_it::expr::Expr::new(std::sync::Arc::new(Self::ExprType::Attribute(
                    $crate::express_it::context::Path::from_type_name::<Self>(
                        $crate::modifier::EffectSubject::Target,
                         "current_value",
                    ),
                )))
            }
            fn parent<S: ExprSchema>() -> $crate::express_it::expr::Expr<Self::Property, S> {
                $crate::express_it::expr::Expr::new(std::sync::Arc::new(Self::ExprType::Attribute(
                    $crate::express_it::context::Path::from_type_name::<Self>(
                        $crate::modifier::EffectSubject::Effect,
                         "current_value",
                    ),
                )))
            }
            fn scoped<S: ExprSchema>(
                subject: impl Into<smol_str::SmolStr>,
            ) -> $crate::express_it::expr::Expr<Self::Property, S> {
                $crate::express_it::expr::Expr::new(std::sync::Arc::new(Self::ExprType::Attribute(
                    $crate::express_it::context::Path::from_type_name::<Self>(subject, "base_value"),
                )))
            }
            fn lit<S: ExprSchema>(
                value: $ValueType,
            ) -> $crate::express_it::expr::Expr<Self::Property, S> {
                $crate::express_it::expr::Expr::<Self::Property, S>::new(std::sync::Arc::new(
                    Self::ExprType::Lit(value),
                ))
            }
            fn set<S: ExprSchema>(
                subject: impl Into<smol_str::SmolStr>,
                expr: impl Into<$crate::express_it::expr::Expr<Self::Property, S>>,
            ) -> $crate::express_it::frame::Assignment<Self::Property, S> {
                $crate::express_it::frame::Assignment {
                    path: $crate::express_it::context::Path::from_type_name::<Self>(subject, "base_value"),
                    expr: expr.into(),
                }
            }
            fn add<S: ExprSchema>(
                subject: impl Into<smol_str::SmolStr> + std::marker::Copy,
                expr: impl Into<$crate::express_it::expr::Expr<Self::Property, S>>,
            ) -> $crate::express_it::frame::Assignment<Self::Property, S> {
                let get_expr = $crate::express_it::expr::Expr::new(std::sync::Arc::new(
                    Self::ExprType::Attribute($crate::express_it::context::Path::from_type_name::<Self>(
                        subject,
                         "base_value",
                    )),
                ));

                $crate::express_it::frame::Assignment {
                    path: $crate::express_it::context::Path::from_type_name::<Self>(subject, "base_value"),
                    expr: get_expr + expr.into(),
                }
            }
            fn sub<S: ExprSchema>(
                subject: impl Into<smol_str::SmolStr> + std::marker::Copy,
                expr: impl Into<$crate::express_it::expr::Expr<Self::Property, S>>,
            ) -> $crate::express_it::frame::Assignment<Self::Property, S> {
                let get_expr = $crate::express_it::expr::Expr::new(std::sync::Arc::new(
                     Self::ExprType::Attribute($crate::express_it::context::Path::from_type_name::<Self>(
                        subject,
                        "base_value",
                    )),
                ));

                $crate::express_it::frame::Assignment {
                    path: $crate::express_it::context::Path::from_type_name::<Self>(subject, "base_value"),
                    expr: get_expr - expr.into(),
                }
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
        #[derive(
            bevy::prelude::Component,
            bevy::prelude::Reflect,
            Default,
            Copy,
            Clone,
            Debug,
            //serde::Serialize,
            //serde::Deserialize,
        )]
        #[reflect(Component)]
        pub struct $StructName;
    };
}

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
    fn any_current_value(&self) -> &dyn Any;
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
    fn any_current_value(&self) -> &dyn Any {
        self.borrow_current_value()
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
