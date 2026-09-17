use anyhow::bail;
use serde::{Deserialize, Serialize};

/// An exact rate. 29.97 fps is 30000/1001, and every frame index in the
/// document leans on it, so it is never divided out until something asks.
///
/// Written to the project file as `[30000, 1001]`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rational(i32, i32);

impl Rational {
    /// Reduced on the way in, so two ways of writing one rate compare equal.
    pub fn new(numer: i32, denom: i32) -> Self {
        Rational(numer, denom).reduced()
    }

    /// Unreduced, for a constant. Give it a reduced pair.
    pub const fn new_raw(numer: i32, denom: i32) -> Self {
        Rational(numer, denom)
    }

    pub fn numer(self) -> i32 {
        self.0
    }

    pub fn denom(self) -> i32 {
        self.1
    }

    pub fn as_f64(self) -> f64 {
        self.0 as f64 / self.1 as f64
    }

    /// The same rate, or why it cannot be one: a file is the one place these
    /// numbers arrive unchecked.
    pub fn checked(self) -> anyhow::Result<Self> {
        let Rational(numer, denom) = self;
        if numer <= 0 || denom <= 0 {
            bail!("{numer}/{denom} is not a usable frame rate");
        }

        Ok(self.reduced())
    }

    fn reduced(self) -> Self {
        let Rational(mut numer, mut denom) = self;
        if denom < 0 {
            (numer, denom) = (-numer, -denom);
        }

        let divisor = gcd(numer.unsigned_abs(), denom.unsigned_abs()).max(1) as i32;

        Rational(numer / divisor, denom / divisor)
    }
}

impl std::fmt::Display for Rational {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.0, self.1)
    }
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
