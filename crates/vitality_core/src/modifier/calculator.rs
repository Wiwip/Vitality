use crate::attributes::Attribute;
use crate::math::SaturatingAttributes;
use bevy::prelude::*;
use num_traits::{AsPrimitive, Bounded, FromPrimitive, Zero};
use serde::Serialize;
use std::fmt::{Debug, Display, Formatter};

#[derive(Debug, Clone, Copy, Reflect, Serialize)]
pub enum ModOp {
    Set,
    Add,
    Sub,
    Increase,
    Mul,
}

impl Display for ModOp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ModOp::Set => write!(f, "="),
            ModOp::Add => write!(f, "+"),
            ModOp::Sub => write!(f, "-"),
            ModOp::Increase => write!(f, "+*"),
            ModOp::Mul => write!(f, "*"),
        }
    }
}

#[derive(Component, Clone, Copy, Reflect, Debug)]
pub struct AttributeCalculatorCached<T: Attribute> {
    #[reflect(ignore)]
    pub calculator: AttributeCalculator<T>,
}

impl<T: Attribute> Default for AttributeCalculatorCached<T> {
    fn default() -> Self {
        Self {
            calculator: AttributeCalculator::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Reflect)]
pub struct AttributeCalculator<T: Attribute> {
    pub(crate) set: Option<T::Property>,
    pub(crate) additive: T::Property,
    pub(crate) subtractive: T::Property,
    pub(crate) increase: f64,
    pub(crate) multiplier: f64,
}

impl<T: Attribute> AttributeCalculator<T> {
    pub fn eval(&self, base_value: T::Property) -> T::Property {
        if let Some(set) = self.set {
            return set;
        }

        let addition_result: T::Property = base_value.saturating_add(self.additive);
        let subtraction_result: f64 = addition_result.saturating_sub(self.subtractive).as_();
        let add_multi_result = subtraction_result * (1.0 + self.increase);
        let result = add_multi_result * self.multiplier;

        let min = T::Property::min_value();
        let max = T::Property::max_value();

        let min_f: f64 = min.as_();
        let max_f: f64 = max.as_();

        let clamped_value = result.clamp(min_f, max_f);
        T::Property::from_f64(clamped_value).unwrap_or_default()
    }

    pub fn combine(self, other: AttributeCalculator<T>) -> AttributeCalculator<T> {
        let set = self.set.or(other.set);
        let additive = self.additive + other.additive;
        let subtractive = self.subtractive + other.subtractive;
        let increase = self.increase + other.increase;
        let multiplier = self.multiplier * other.multiplier;

        AttributeCalculator::<T> {
            set,
            additive,
            subtractive,
            increase,
            multiplier,
        }
    }

    pub fn combine_in_place(&mut self, other: &AttributeCalculator<T>) {
        self.set = self.set.or(other.set);
        self.additive += other.additive;
        self.subtractive += other.subtractive;
        self.increase += other.increase;
        self.multiplier *= other.multiplier;
    }

    pub fn from_parts(
        set: Option<T::Property>,
        additive: T::Property,
        subtractive: T::Property,
        increase: f64,
        multiplier: f64,
    ) -> Self {
        Self {
            set,
            additive,
            subtractive,
            increase,
            multiplier,
        }
    }

    /// One modifier applied `stacks` times: additive ops scale, `Mul` compounds, `Set` ignores it.
    pub fn from_op_stacked(operation: ModOp, value: T::Property, stacks: u32) -> Self {
        if stacks <= 1 {
            return Self::from_op(operation, value);
        }
        match operation {
            ModOp::Set => Self::from_op(operation, value),
            ModOp::Mul => Self {
                multiplier: value.as_().powi(stacks as i32),
                ..default()
            },
            ModOp::Increase => Self {
                increase: value.as_() * stacks as f64,
                ..default()
            },
            ModOp::Add | ModOp::Sub => {
                let scaled = T::Property::from_f64(value.as_() * stacks as f64)
                    .unwrap_or_else(T::Property::max_value);
                Self::from_op(operation, scaled)
            }
        }
    }

    pub fn from_op(operation: ModOp, value: T::Property) -> Self {
        match operation {
            ModOp::Set => Self {
                set: Some(value),
                ..default()
            },
            ModOp::Add => Self {
                additive: value,
                ..default()
            },
            ModOp::Sub => Self {
                subtractive: value,
                ..default()
            },
            ModOp::Increase => Self {
                increase: value.as_(),
                ..default()
            },
            ModOp::Mul => Self {
                multiplier: value.as_(),
                ..default()
            },
        }
    }
}

impl<T: Attribute> Default for AttributeCalculator<T> {
    fn default() -> Self {
        Self {
            set: None,
            additive: T::Property::zero(),
            subtractive: T::Property::zero(),
            increase: 0.0,
            multiplier: 1.0,
        }
    }
}
