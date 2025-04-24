use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Credits(pub i64);

impl Credits {
    pub fn new(amount: i64) -> Self {
        Credits(amount)
    }

    pub fn amount(&self) -> i64 {
        self.0
    }

    pub fn is_positive(&self) -> bool {
        self.0 > 0
    }

    pub fn is_negative(&self) -> bool {
        self.0 < 0
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub fn abs(&self) -> Self {
        Credits(self.0.abs())
    }
}

// Basic arithmetic operations
impl Add for Credits {
    type Output = Credits;

    fn add(self, other: Credits) -> Credits {
        Credits(self.0 + other.0)
    }
}

impl AddAssign for Credits {
    fn add_assign(&mut self, other: Credits) {
        self.0 += other.0;
    }
}

impl Sub for Credits {
    type Output = Credits;

    fn sub(self, other: Credits) -> Credits {
        Credits(self.0 - other.0)
    }
}

impl SubAssign for Credits {
    fn sub_assign(&mut self, other: Credits) {
        self.0 -= other.0;
    }
}

impl Neg for Credits {
    type Output = Credits;

    fn neg(self) -> Credits {
        Credits(-self.0)
    }
}

// Operations with i64
impl Add<i64> for Credits {
    type Output = Credits;

    fn add(self, other: i64) -> Credits {
        Credits(self.0 + other)
    }
}

impl Sub<i64> for Credits {
    type Output = Credits;

    fn sub(self, other: i64) -> Credits {
        Credits(self.0 - other)
    }
}

// From/Into conversions
impl From<i64> for Credits {
    fn from(amount: i64) -> Self {
        Credits(amount)
    }
}

impl From<Credits> for i64 {
    fn from(credits: Credits) -> Self {
        credits.0
    }
}

// Display implementation for nice formatting
impl fmt::Display for Credits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}c", self.0)
    }
}

impl std::ops::Mul<u32> for Credits {
    type Output = Credits;

    fn mul(self, quantity: u32) -> Credits {
        Credits(self.0 * i64::from(quantity))
    }
}

impl std::ops::Mul<i32> for Credits {
    type Output = Credits;

    fn mul(self, quantity: i32) -> Credits {
        Credits(self.0 * i64::from(quantity))
    }
}

impl std::ops::Div<i32> for Credits {
    type Output = Credits;

    fn div(self, rhs: i32) -> Self::Output {
        Credits(self.0 * i64::from(rhs))
    }
}
