pub type Range<T> = (T, T);

// Could not find a simple library for this.
pub fn ranges_intersect<T: Ord>(a: Range<T>, b: Range<T>) -> bool {
    a.0 < b.1 && b.0 < a.1
}

pub fn range_contains<T: Ord>(r: Range<T>, x: T) -> bool {
    r.0 <= x && x < r.1
}

pub fn is_ordered<T: Ord>(seq: &Vec<T>) -> bool {
    for (a, b) in seq.iter().zip(seq.iter().skip(1)) {
        if a > b {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_is_ordered() {
        assert!(is_ordered::<u64>(&vec![]));
        assert!(is_ordered(&vec![0]));
        assert!(!is_ordered(&vec![3, 2]));
        assert!(is_ordered(&vec![2, 3]));
        assert!(is_ordered(&vec![2, 2]));
        assert!(!is_ordered(&vec![2, 3, 1]));
        assert!(is_ordered(&vec![2, 3, 3]));
    }
}
