use crate::value::AttributeValue;
use bevy_reflect::prelude::*;
use bevy_utils::default;
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

/// One attribute's folded modifiers: `(base + Σadd − Σsub) × (1 + Σincrease) × Πmul`, or `set`.
#[derive(Clone, Copy, Debug)]
pub struct Calc<P: AttributeValue> {
    pub set: Option<P>,
    pub add: P,
    pub sub: P,
    pub increase: P,
    pub mul: P,
}

impl<P: AttributeValue> Default for Calc<P> {
    fn default() -> Self {
        Self {
            set: None,
            add: P::zero(),
            sub: P::zero(),
            increase: P::zero(),
            mul: P::one(),
        }
    }
}

impl<P: AttributeValue> Calc<P> {
    pub fn from_op(operation: ModOp, value: P) -> Self {
        match operation {
            ModOp::Set => Self {
                set: Some(value),
                ..default()
            },
            ModOp::Add => Self {
                add: value,
                ..default()
            },
            ModOp::Sub => Self {
                sub: value,
                ..default()
            },
            ModOp::Increase => Self {
                increase: value,
                ..default()
            },
            ModOp::Mul => Self {
                mul: value,
                ..default()
            },
        }
    }

    /// sums scale by `stacks`, `Mul` compounds, `Set` ignores it
    pub fn from_op_stacked(operation: ModOp, value: P, stacks: u32) -> Self {
        let stacks = stacks.max(1);
        if stacks == 1 {
            return Self::from_op(operation, value);
        }
        match operation {
            ModOp::Set => Self::from_op(operation, value),
            ModOp::Mul => Self {
                mul: value.powi(stacks),
                ..default()
            },
            ModOp::Increase => Self {
                increase: scale(value, stacks),
                ..default()
            },
            ModOp::Add | ModOp::Sub => {
                let scaled = scale(value, stacks);
                Self::from_op(operation, scaled)
            }
        }
    }

    /// the first `set` in walk order wins
    pub fn combine(&mut self, other: &Self) {
        self.set = self.set.or(other.set);
        self.add = P::add(self.add, other.add);
        self.sub = P::add(self.sub, other.sub);
        self.increase = P::add(self.increase, other.increase);
        self.mul = P::mul(self.mul, other.mul);
    }

    pub fn eval(&self, base_value: P) -> P {
        if let Some(set) = self.set {
            return set;
        }
        let sum = P::sub(P::add(base_value, self.add), self.sub);
        let grown = P::mul(sum, P::add(P::one(), self.increase));
        P::mul(grown, self.mul)
    }
}

fn scale<P: AttributeValue>(value: P, count: u32) -> P {
    P::mul(value, P::from_f64(count as f64))
}
