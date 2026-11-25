use crate::context::{ActorProvider, ActorProviderMut, Caster, Source, Target};
use crate::util::pretty_type_name;
use crate::value::AttributeValue;
use crate::view::{ActorRead, ActorWrite};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::{Bundle, Component, Entity};
use bevy_ecs::reflect::ReflectComponent;
use bevy_reflect::{GetTypeRegistration, Typed, reflect_trait};
use bevy_reflect::{Reflect, TypePath};
use express_it::expr::{AsExpression, Context, ContextMut, DepKey, Deps, Expr};
use express_it::nodes::Node;
use express_it::plan::AssignmentStep;
use std::any::TypeId;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::marker::PhantomData;
use tracing::error;

/// [`AttributeValue`] for the fold, plus what reflection and `attribute!` need.
pub trait GameValue:
    AttributeValue + Default + Debug + Display + Typed + GetTypeRegistration
{
}

impl<T> GameValue for T where
    T: AttributeValue + Default + Debug + Display + Typed + GetTypeRegistration
{
}

/// The `Node` an attribute read compiles to; spelled out it sits over clippy's complexity limit.
type AttributeNode<A, P, C, Role> = Node<P, C, AttributeVar<A, P, C, Role>>;

/// setter half of an assignment step, so `add`/`sub`/`set` don't spell a bare `impl Fn` in return position
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

    fn new(value: Self::Property) -> Self;
    /// the component as a mere carrier starts out: base and current at the property's default
    fn zeroed() -> Self;
    fn base_value(&self) -> Self::Property;
    fn base(&self) -> Self::Property;
    fn set_base_value(&mut self, value: Self::Property);
    /// the base the last fold turned into `current_value`, so the base-write scan compares in place
    fn folded_base(&self) -> Option<Self::Property>;
    fn set_folded_base(&mut self, value: Self::Property);
    fn current_value(&self) -> Self::Property;
    fn val(&self) -> Self::Property;
    fn set_current_value(&mut self, value: Self::Property);

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
        Node::from_expr(AttributeVar {
            fetch_fn: |ctx: &C::ContextItem<'_, '_>| match ctx.get_actor().get::<Self>() {
                Some(attribute) => attribute.current_value(),
                None => missing::<Self, Role>(),
            },
            _marker: Default::default(),
        })
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

        AssignmentStep::new(
            |ctx: &mut C::ContextItemMut<'_, '_>, expr_val: Self::Property| {
                let actor = ActorProviderMut::<Role>::get_actor_mut(ctx);
                match actor.get_mut::<Self>() {
                    None => {
                        error!(
                            "{} not found on the actor being written: that assignment is \
                             dropped, put the component on the actor",
                            pretty_type_name::<Self>()
                        )
                    }
                    Some(mut attr) => {
                        let base = attr.base();
                        attr.set_base_value(<Self::Property as AttributeValue>::add(base, expr_val))
                    }
                }
            },
            expr,
            None,
        )
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

        AssignmentStep::new(
            |ctx: &mut C::ContextItemMut<'_, '_>, expr_val: Self::Property| {
                let actor = ActorProviderMut::<Role>::get_actor_mut(ctx);
                match actor.get_mut::<Self>() {
                    None => {
                        error!(
                            "{} not found on the actor being written: that assignment is \
                             dropped, put the component on the actor",
                            pretty_type_name::<Self>()
                        )
                    }
                    Some(mut attr) => {
                        let base = attr.base();
                        attr.set_base_value(<Self::Property as AttributeValue>::sub(base, expr_val))
                    }
                }
            },
            expr,
            None,
        )
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

        AssignmentStep::new(
            |ctx: &mut C::ContextItemMut<'_, '_>, expr_val: Self::Property| {
                let actor = ActorProviderMut::<Role>::get_actor_mut(ctx);
                match actor.get_mut::<Self>() {
                    None => {
                        error!(
                            "{} not found on the actor being written: that assignment is \
                             dropped, put the component on the actor",
                            pretty_type_name::<Self>()
                        )
                    }
                    Some(mut attr) => attr.set_base_value(expr_val),
                }
            },
            expr,
            None,
        )
    }
}

/// A read of an attribute the actor doesn't have counts as zero.
#[cold]
fn missing<T: Attribute, Role>() -> T::Property {
    crate::warn_once!(
        "{} not found on {}; reading it as zero: put the component on the actor, or stop \
         reading it there",
        pretty_type_name::<T>(),
        pretty_type_name::<Role>()
    );
    T::Property::default()
}

#[macro_export]
macro_rules! attribute_impl {
    ( $StructName:ident, $ValueType:ty ) => {
        #[derive(
            $crate::bevy_ecs::prelude::Component,
            Clone,
            Copy,
            $crate::bevy_reflect::Reflect,
            Debug,
            $crate::bevy_ecs::prelude::FromTemplate,
            $crate::bevy_derive::Deref,
        )]
        #[component(summary_tick)]
        #[reflect(Component, AccessAttribute)]
        pub struct $StructName {
            pub base_value: $ValueType,
            #[deref]
            current_value: $ValueType,
            folded_base: Option<$ValueType>,
        }

        impl $crate::attributes::Attribute for $StructName {
            type Property = $ValueType;

            fn new(value: $ValueType) -> Self {
                Self {
                    base_value: value,
                    current_value: value,
                    folded_base: None,
                }
            }
            fn zeroed() -> Self {
                Self {
                    base_value: Default::default(),
                    current_value: Default::default(),
                    folded_base: None,
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
            fn folded_base(&self) -> Option<$ValueType> {
                self.folded_base
            }
            fn set_folded_base(&mut self, value: $ValueType) {
                self.folded_base = Some(value);
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

/// Called once per attribute of a set, so the caller does the same thing to every member.
pub trait AttributeVisitor {
    fn visit<T: Attribute>(&mut self);
}

/// An `attribute_set!` bundle: its members are known without an instance of it.
pub trait AttributeSet: Bundle {
    fn visit_members(visitor: &mut impl AttributeVisitor);
}

#[macro_export]
macro_rules! attribute_set {
    ( $(#[$meta:meta])* $vis:vis struct $Name:ident { $( $Attr:ident : $Ty:ty ),* $(,)? } ) => {
        $( $crate::attribute_impl!($Attr, $Ty); )*

        // the fields and `new`'s params keep the caller's case: macro_rules cannot convert it
        #[allow(non_snake_case)]
        $(#[$meta])*
        #[derive($crate::bevy_ecs::prelude::Bundle, Clone)]
        $vis struct $Name {
            $( $vis $Attr: $Attr, )*
        }

        #[allow(non_snake_case)]
        impl $Name {
            $vis fn new($( $Attr: $Ty ),*) -> Self {
                Self {
                    $( $Attr: <$Attr as $crate::attributes::Attribute>::new($Attr), )*
                }
            }
        }

        // derived `Default` would need it on the members, which `attribute_impl!` does not give
        impl ::core::default::Default for $Name {
            fn default() -> Self {
                Self {
                    $( $Attr: <$Attr as $crate::attributes::Attribute>::zeroed(), )*
                }
            }
        }

        impl $crate::attributes::AttributeSet for $Name {
            fn visit_members(visitor: &mut impl $crate::attributes::AttributeVisitor) {
                $( visitor.visit::<$Attr>(); )*
            }
        }
    };
}

#[macro_export]
macro_rules! tag {
    ( $StructName:ident ) => {
        #[derive(
            $crate::bevy_ecs::prelude::Component,
            $crate::bevy_reflect::Reflect,
            Default,
            Copy,
            Clone,
            Debug,
            $crate::bevy_ecs::prelude::FromTemplate,
        )]
        #[reflect(Component)]
        pub struct $StructName;
    };
}

#[derive(Component, Default, Debug, Copy, Clone)]
#[require(crate::registry::ModifierTable)]
pub struct ManageAttributes;

/// bounds on an attribute's base and current value, kept current by the facade from the actor's clamps
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
            return value;
        }
        if self.min_limit > self.max_limit {
            crate::warn_once!(
                "{} is clamped between min {} and max {}, which is inverted: every value lands on \
                 the max, swap the two limits to bound it from below",
                pretty_type_name::<T>(),
                self.min_limit,
                self.max_limit,
            );
        }
        // comparisons rather than max/min, so a NaN value still falls through both bounds untouched
        let raised = if value < self.min_limit {
            self.min_limit
        } else {
            value
        };
        if raised > self.max_limit {
            self.max_limit
        } else {
            raised
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
        self.base_value().to_f64()
    }
    fn access_current_value(&self) -> f64 {
        self.current_value().to_f64()
    }
    fn name(&self) -> String {
        pretty_type_name::<T>()
    }
}

/// Dependency key of `T` read through `Role`, as reported by [`Expr::get_dependencies`].
struct AttributeRead<T, Role>(PhantomData<fn() -> (T, Role)>);

pub fn dep_key<T: 'static, Role: 'static>() -> TypeId {
    TypeId::of::<AttributeRead<T, Role>>()
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
        (self.fetch_fn)(ctx)
    }

    fn get_dependencies(&self, deps: &mut Deps) {
        deps.insert(DepKey::Type(dep_key::<T, Role>()));
    }
}
impl<T, N, C: Context, Role> Copy for AttributeVar<T, N, C, Role> {}
impl<T, N: fmt::Display, C: Context, Role> fmt::Display for AttributeVar<T, N, C, Role> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", pretty_type_name::<T>())
    }
}

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

#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = AttributeDependency<T>)]
pub struct AttributeDependents<T: Attribute + 'static> {
    #[relationship]
    sources: Vec<Entity>,
    marker: PhantomData<T>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ActorExprContext, ActorExprSchema};
    use crate::{AttributeBindings, DepRole};
    use bevy_ecs::prelude::World;
    use std::collections::HashSet;

    crate::attribute!(Attack);
    crate::attribute!(Defense);
    crate::attribute!(Haste);

    fn world_with(attack: f32, defense: f32, haste: f32) -> (World, Entity) {
        let mut world = World::new();
        let actor = world
            .spawn((
                Attack::new(attack),
                Defense::new(defense),
                Haste::new(haste),
            ))
            .id();
        (world, actor)
    }

    fn ctx(world: &World, actor: Entity) -> ActorExprContext<'_, '_> {
        ActorExprContext {
            actor_context: world.entity(actor).into(),
        }
    }

    #[test]
    fn a_composite_expression_forwards_every_attribute_it_reads() {
        let (world, actor) = world_with(3.0, 4.0, 2.0);
        let ctx = ctx(&world, actor);
        let expr = (Attack::src::<ActorExprSchema>() + Defense::src::<ActorExprSchema>())
            * Haste::src::<ActorExprSchema>();

        assert_eq!(expr.eval(&ctx), 14.0, "the walk leaves the eval alone");

        let deps = expr.dependencies();
        assert_eq!(
            deps.iter().collect::<HashSet<DepKey>>(),
            HashSet::from([
                DepKey::Type(dep_key::<Attack, Source>()),
                DepKey::Type(dep_key::<Defense, Source>()),
                DepKey::Type(dep_key::<Haste, Source>()),
            ]),
            "the walk is a may-read superset, one key per (attribute, read role): arithmetic nodes \
             are exact, `&&`/`||` report both operands because their eval short-circuits"
        );
    }

    // the modifier spawn half lives in vitality_effects, so this pins the binding contract it reads
    #[test]
    fn a_registered_expressions_captured_dependencies_all_resolve_in_the_bindings() {
        let mut bindings = AttributeBindings::default();
        bindings.add::<Attack>();
        bindings.add::<Defense>();

        let walked =
            (Attack::src::<ActorExprSchema>() + Defense::src::<ActorExprSchema>()).dependencies();
        // an edge can only point at an attribute, so only the type keys the walk reports are bindings
        let deps: Vec<TypeId> = walked.iter().filter_map(DepKey::as_type_id).collect();

        let resolved: Vec<(TypeId, DepRole)> = deps
            .iter()
            .map(|key| {
                bindings.dependency(key).unwrap_or_else(|| {
                    panic!("captured dependency {key:?} is not a registered attribute binding")
                })
            })
            .map(|binding| (binding.attribute, binding.role))
            .collect();

        assert_eq!(
            resolved.len(),
            deps.len(),
            "a modifier spawns one dependency edge per read, so an unresolved key drops an edge"
        );
        assert_eq!(
            resolved.into_iter().collect::<HashSet<_>>(),
            HashSet::from([
                (TypeId::of::<Attack>(), DepRole::Source),
                (TypeId::of::<Defense>(), DepRole::Source),
            ]),
            "the registered set has to name the same attributes the expression reads, read as the source"
        );
    }
}
