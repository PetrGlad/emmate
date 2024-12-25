use num::{Integer, Num};

/// Half-open range [a, b).
/// My favorite implementation until, maybe,
/// https://github.com/rust-lang/rfcs/pull/3550 is implemented.
/// The trait impl and a tuple should be enough, this type alias helps to clarify intent.
pub type Range<T> = (T, T);

pub trait RangeLike<T> {
    fn intersects(&self, other: &Self) -> bool;
    fn contains(&self, x: &T) -> bool;
    fn is_empty(&self) -> bool;
    fn range(&self) -> std::ops::Range<T>;
    fn from(from: std::ops::Range<T>) -> Self;
}

impl<T: Ord + Copy> RangeLike<T> for Range<T> {
    fn intersects(&self, other: &Self) -> bool {
        self.0 <= other.1 && other.0 < self.1
    }

    fn contains(&self, x: &T) -> bool {
        self.0 <= *x && *x < self.1
    }

    fn is_empty(&self) -> bool {
        self.1 <= self.0
    }

    fn range(&self) -> std::ops::Range<T> {
        self.0..self.1
    }

    fn from(from: std::ops::Range<T>) -> Self {
        (from.start, from.end)
    }
}

pub trait RangeSpan<T> {
    fn len(&self) -> usize;
}

impl<T: Num + Copy + Into<usize>> RangeSpan<T> for Range<T> {
    fn len(&self) -> usize {
        (self.1 - self.0).into()
    }
}

pub fn closed_range<T: Integer>(from: T, to: T) -> Range<T> {
    (from, to + T::one())
}

pub fn as_closed<T: Integer + Copy>(range: &Range<T>) -> std::ops::RangeInclusive<T> {
    range.0..=(range.1 - T::one())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_range_contains() {
        assert!(!(0, -1).contains(&0));
        assert!(!(0, 0).contains(&0));
        assert!((0, 1).contains(&0));
        assert!(!(0, 1).contains(&1));
        assert!(!(0, 1).contains(&2));
    }

    #[test]
    fn check_ranges_intersect() {
        assert!((0, 0).is_empty());
        assert!(!(0, 1).is_empty());
        assert!(!(0, 0).intersects(&(0, 1))); // Empty
        assert!(!(0, 1).intersects(&(1, 2))); // End is not included

        assert!((0, 1).intersects(&(0, 1)));
        assert!((0, 1).intersects(&(0, 2)));
        assert!((0, 1).intersects(&(-1, 0))); // Start is not included
        assert!((0, 1).intersects(&(-1, 1)));
        assert!((0, 1).intersects(&(-1, 2)));
        assert!((-1, 2).intersects(&(0, 1)));
    }
}
