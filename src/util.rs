use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::de::DeserializeOwned;
use serde::Serialize;

pub type Range<T> = (T, T);

// Could not find a simple library for this.
#[allow(dead_code)]
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

#[derive(Debug, Default)]
pub struct IdSeq(atomic::AtomicU64);

impl IdSeq {
    pub fn new(init: u64) -> Self {
        IdSeq(atomic::AtomicU64::new(init))
    }

    pub fn next(&mut self) -> u64 {
        self.0.fetch_add(1, atomic::Ordering::SeqCst)
    }

    pub fn current(&self) -> u64 {
        self.0.load(atomic::Ordering::SeqCst)
    }
}

pub fn load<T: DeserializeOwned>(file_path: &PathBuf) -> T {
    let binary = std::fs::read(file_path).expect(&*format!("load from {}", &file_path.display()));
    let mut decoder = GzDecoder::new(binary.as_slice());
    let mut binary = vec![];
    decoder.read_to_end(&mut binary).expect("unzip serialized");
    rmp_serde::from_slice(&binary).expect("deserialize")
}

pub fn store<T: Serialize>(x: &T, file_path: &PathBuf) {
    let mut binary = Vec::new();
    x.serialize(
        // TODO If using compact representation (without field names), add some format version info in the data and/or in file names.
        //   Consider using protobuf.
        &mut rmp_serde::Serializer::new(&mut binary), /*.with_struct_map()*/
    )
    .expect("serialize");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&binary.as_slice())
        .expect("gzip serialized");
    let binary = encoder.finish().expect("gzip serialized");
    std::fs::write(file_path, &binary).expect(&*format!("write to {}", &file_path.display()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_range_contains() {
        assert!(!range_contains((0, -1), 0));
        assert!(!range_contains((0, 0), 0));
        assert!(range_contains((0, 1), 0));
        assert!(!range_contains((0, 1), 1));
        assert!(!range_contains((0, 1), 2));
    }

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
