use crate::context::{ActorProvider, ActorProviderMut, Caster, Source, Target};
use crate::math::{AbsDiff, SaturatingAttributes};
use crate::modifier::{AttributeCalculator, AttributeCalculatorCached};
use crate::util::pretty_type_name;
use crate::view::{ActorRead, ActorWrite};
use bevy::ecs::component::Mutable;
use bevy::ecs::query::QueryData;
use bevy::log::error;
use bevy::prelude::{Component, Entity, Reflect, ReflectComponent, TypePath};
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

/// The `Node` an attribute read compiles to; spelled out it sits over clippy's complexity limit.
type AttributeNode<A, P, C, Role> = Node<P, C, AttributeVar<A, P, C, Role>>;

/// The setter half of an assignment step, named so `add`/`sub`/`set` do not spell a bare `impl Fn`
/// in return position. Blanket-implemented, so it constrains nothing the bare form did not.
pub trait Setter<P, C: ContextMut>:
    Fn(&mut C::ContextItemMut<'_, '_>, P) + 'static + Send + Sync
{
}

impl<F, P, C> Setter<P, C> for F
where
    F: Fn(&mut C::ContextItemMut<'_, '_>, P) + 'static + Send + Sync,
    C: ContextMut,
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
    fn src<C>() -> AttributeNode<Self, Self::Property, C, Source>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>: ActorProvider<'a, 'b, Source>,
    {
        Self::at::<Source, C>()
    }
    fn dst<C>() -> AttributeNode<Self, Self::Property, C, Target>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>: ActorProvider<'a, 'b, Target>,
    {
        Self::at::<Target, C>()
    }
    fn caster<C>() -> AttributeNode<Self, Self::Property, C, Caster>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>: ActorProvider<'a, 'b, Caster>,
    {
        Self::at::<Caster, C>()
    }
    fn at<Role: 'static, C>() -> AttributeNode<Self, Self::Property, C, Role>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>: ActorProvider<'a, 'b, Role>,
    {
        Node {
            expr: AttributeVar {
                fetch_fn: |ctx: &C::ContextItem<'_, '_>| match ctx.get_actor().get::<Self>() {
                    Some(attribute) => attribute.current_value(),
                    None => missing::<Self, Role>(),
                },
                _marker: Default::default(),
            },
            _marker: Default::default(),
        }
    }
    fn at_base<Role: 'static, C>() -> AttributeNode<Self, Self::Property, C, Role>
    where
        C: Context,
        for<'a, 'b> C::ContextItem<'a, 'b>: ActorProvider<'a, 'b, Role>,
    {
        Node {
            expr: AttributeVar {
                fetch_fn: |ctx: &C::ContextItem<'_, '_>| match ctx.get_actor().get::<Self>() {
                    Some(attribute) => attribute.base_value(),
                    None => missing::<Self, Role>(),
                },
                _marker: Default::default(),
            },
            _marker: Default::default(),
        }
    }

    fn add<Role, C>(
        expr: impl AsExpression<Self::Property, C, Target: Copy + Send + Sync + 'static>,
    ) -> AssignmentStep<
        Self::Property,
        impl Expr<Self::Property, C> + Copy + Send + Sync + 'static,
        impl Setter<Self::Property, C>,
    >
    where
        Role: 'static,
        C: ContextMut + 'static,
        for<'a, 'b> C::ContextItemMut<'a, 'b>: ActorProviderMut<'a, 'b, Role>,
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

    fn sub<Role, C>(
        expr: impl AsExpression<Self::Property, C, Target: Copy + Send + Sync + 'static>,
    ) -> AssignmentStep<
        Self::Property,
        impl Expr<Self::Property, C> + Copy + Send + Sync + 'static,
        impl Setter<Self::Property, C>,
    >
    where
        Role: 'static,
        C: ContextMut + 'static,
        for<'a, 'b> C::ContextItemMut<'a, 'b>: ActorProviderMut<'a, 'b, Role>,
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

    fn set<Role, C>(
        expr: impl AsExpression<Self::Property, C, Target: Copy + Send + Sync + 'static>,
    ) -> AssignmentStep<
        Self::Property,
        impl Expr<Self::Property, C> + Copy + Send + Sync + 'static,
        impl Setter<Self::Property, C>,
    >
    where
        Role: 'static,
        C: ContextMut + 'static,
        for<'a, 'b> C::ContextItemMut<'a, 'b>: ActorProviderMut<'a, 'b, Role>,
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

/// A read of an attribute the actor doesn't have counts as zero.
#[cold]
fn missing<T: Attribute, Role>() -> T::Property {
    bevy::log::warn_once!(
        "{} not found on {}; reading it as zero",
        pretty_type_name::<T>(),
        pretty_type_name::<Role>()
    );
    T::Property::default()
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
        #[derive(
            bevy::prelude::Component,
            bevy::prelude::Reflect,
            Default,
            Copy,
            Clone,
            Debug,
            bevy::prelude::FromTemplate,
        )]
        #[reflect(Component)]
        pub struct $StructName;
    };
}

#[derive(Component, Default, Debug, Copy, Clone)]
#[require(crate::registry::ModifierTable)]
pub struct ManageAttributes;

/// Bounds on an attribute's base and current value, kept up to date by the facade from the
/// actor definition's clamp expressions.
#[derive(Component, Default, Debug, Clone, Reflect)]
#[reflect(Component, from_reflect = false)]
pub struct Clamp<T: Attribute> {
    pub min_limit: T::Property,
    pub max_limit: T::Property,
    /// False until the limits are first evaluated; an unready clamp bounds nothing.
    pub ready: bool,
}

impl<T: Attribute> Clamp<T> {
    pub fn new() -> Self {
        Self {
            min_limit: T::Property::default(),
            max_limit: T::Property::default(),
            ready: false,
        }
    }

    pub fn apply(&self, value: T::Property) -> T::Property {
        if !self.ready {
            value
        } else if value < self.min_limit {
            self.min_limit
        } else if value > self.max_limit {
            self.max_limit
        } else {
            value
        }
    }
}

pub type ClampExpr<T> = std::sync::Arc<
    dyn Expr<<T as Attribute>::Property, crate::context::ActorExprSchema> + Send + Sync,
>;

/// The expressions a [`Clamp`]'s limits come from, evaluated against the actor itself.
#[derive(Component, Clone)]
pub struct ClampBounds<T: Attribute> {
    pub min: ClampExpr<T>,
    pub max: ClampExpr<T>,
}

#[derive(QueryData, Debug)]
#[query_data(mutable, derive(Debug))]
pub struct AttributeQueryData<T: Attribute + 'static> {
    pub entity: Entity,
    pub attribute: &'static mut T,
    pub calculator_cache: &'static mut AttributeCalculatorCached<T>,
    pub clamp: Option<&'static Clamp<T>>,
}

impl<T: Attribute> AttributeQueryDataItem<'_, '_, T> {
    pub fn update_attribute(&mut self, calculator: &AttributeCalculator<T>) -> bool {
        let old_val = self.attribute.current_value();
        let mut new_val = calculator.eval(self.attribute.base_value());
        if let Some(clamp) = self.clamp {
            new_val = clamp.apply(new_val);
        }

        let has_changed = old_val.are_different(new_val);
        if has_changed {
            self.attribute.set_current_value(new_val);
        }
        has_changed
    }

    pub fn update_attribute_from_cache(&mut self) -> bool {
        let old_val = self.attribute.current_value();
        let mut new_val = self
            .calculator_cache
            .calculator
            .eval(self.attribute.base_value());
        if let Some(clamp) = self.clamp {
            new_val = clamp.apply(new_val);
        }

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

thread_local! {
    static RECORDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static RECORDED: std::cell::RefCell<HashSet<TypeId>> = std::cell::RefCell::new(HashSet::new());
}

/// Runs `f` and returns the dependency keys of every attribute it read. express-it's operator
/// nodes don't forward `get_dependencies`, so a composite expression only reports through this.
pub fn record_dependencies<R>(f: impl FnOnce() -> R) -> (R, HashSet<TypeId>) {
    let was_recording = RECORDING.replace(true);
    let out = f();
    RECORDING.set(was_recording);
    let deps = RECORDED.with_borrow_mut(std::mem::take);
    (out, deps)
}

#[cold]
fn note_read(key: TypeId) {
    RECORDED.with_borrow_mut(|deps| deps.insert(key));
}

/// Dependency key of `T` read through `Role`, as reported by [`Expr::get_dependencies`].
pub struct DepKey<T, Role>(PhantomData<fn() -> (T, Role)>);

pub fn dep_key<T: 'static, Role: 'static>() -> TypeId {
    TypeId::of::<DepKey<T, Role>>()
}

pub struct AttributeVar<T, N, C: Context, Role = Source> {
    pub fetch_fn: for<'w, 's> fn(&C::ContextItem<'w, 's>) -> N,
    _marker: PhantomData<fn() -> (T, Role)>,
}
impl<T, N, C: Context, Role> Clone for AttributeVar<T, N, C, Role> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static, N: 'static, C: Context + 'static, Role: 'static> Expr<N, C>
    for AttributeVar<T, N, C, Role>
{
    #[inline(always)]
    fn eval(&self, ctx: &C::ContextItem<'_, '_>) -> N {
        if RECORDING.get() {
            note_read(dep_key::<T, Role>());
        }
        (self.fetch_fn)(ctx)
    }

    fn get_dependencies(&self, deps: &mut HashSet<TypeId>) {
        deps.insert(dep_key::<T, Role>());
    }
}
impl<T, N, C: Context, Role> Copy for AttributeVar<T, N, C, Role> {}
impl<T, N: fmt::Display, C: Context, Role> fmt::Display for AttributeVar<T, N, C, Role> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", pretty_type_name::<T>())
    }
}

/// What are the attributes the modifier depends on?
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = AttributeDependents<T>)]
pub struct AttributeDependency<T: Attribute + 'static> {
    #[relationship]
    pub source: Entity,
    marker: PhantomData<T>,
}

impl<T: Attribute> AttributeDependency<T> {
    pub fn new(source: Entity) -> Self {
        Self {
            source,
            marker: PhantomData,
        }
    }
}

/// Usually on actors. Who depends on this entity and for what attributes?
/// Serves to notify derived modifiers that the attribute has changed.
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = AttributeDependency<T>)]
pub struct AttributeDependents<T: Attribute + 'static> {
    #[relationship]
    sources: Vec<Entity>,
    marker: PhantomData<T>,
}
