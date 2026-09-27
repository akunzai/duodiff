//! Left and right. Everything a session holds once per side — a root and its
//! ignore rules, a file of a file pair, a working buffer — is a [`Pair`], so
//! the two sides move together and a caller names a side with [`Side`] rather
//! than mirroring every field and branch by hand (Issue #237 swapped the roots
//! but not their ignore matchers).

/// One side of a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// Both sides, left first.
    pub const BOTH: [Side; 2] = [Side::Left, Side::Right];

    /// The side across from this one.
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// One value for each side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pair<T> {
    pub left: T,
    pub right: T,
}

impl<T> Pair<T> {
    pub fn new(left: T, right: T) -> Self {
        Self { left, right }
    }

    /// The value `f` gives for each side, left first.
    pub fn from_fn(mut f: impl FnMut(Side) -> T) -> Self {
        let left = f(Side::Left);
        Self::new(left, f(Side::Right))
    }

    /// The value on `side`.
    pub fn side(&self, side: Side) -> &T {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    /// The value on `side`, to change.
    pub fn side_mut(&mut self, side: Side) -> &mut T {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    /// Put each side's value on the other side.
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
    }

    /// Borrow both values.
    pub fn as_ref(&self) -> Pair<&T> {
        Pair::new(&self.left, &self.right)
    }

    /// Turn each side's value into another, left first.
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> Pair<U> {
        let left = f(self.left);
        Pair::new(left, f(self.right))
    }

    /// Turn each side's value into another, left first, stopping at the first
    /// error.
    pub fn try_map<U, E>(self, mut f: impl FnMut(T) -> Result<U, E>) -> Result<Pair<U>, E> {
        let left = f(self.left)?;
        Ok(Pair::new(left, f(self.right)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_side_reads_and_writes_its_own_value() {
        let mut pair = Pair::new("l", "r");
        assert_eq!(*pair.side(Side::Left), "l");
        assert_eq!(*pair.side(Side::Right), "r");
        *pair.side_mut(Side::Right) = "R";
        assert_eq!(pair, Pair::new("l", "R"));
        assert_eq!(Side::Left.other(), Side::Right);
        assert_eq!(
            Pair::from_fn(|side| side.other()),
            Pair::new(Side::Right, Side::Left)
        );
    }

    #[test]
    fn swap_moves_both_values_across() {
        let mut pair = Pair::new(1, 2);
        pair.swap();
        assert_eq!(pair, Pair::new(2, 1));
    }

    #[test]
    fn try_map_stops_at_the_left_error() {
        let mut seen = Vec::new();
        let result: Result<Pair<i32>, &str> = Pair::new("l", "r").try_map(|side| {
            seen.push(side);
            Err(side)
        });
        assert_eq!(result, Err("l"));
        assert_eq!(seen, ["l"]);
        assert_eq!(Pair::new(1, 2).map(|n| n * 10), Pair::new(10, 20));
    }
}
