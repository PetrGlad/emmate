use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Default)]
pub struct IdSeq(atomic::AtomicU64);

impl IdSeq {
    pub fn new(init: u64) -> Self {
        IdSeq(atomic::AtomicU64::new(init))
    }

    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, atomic::Ordering::SeqCst)
    }

    pub fn current(&self) -> u64 {
        self.0.load(atomic::Ordering::SeqCst)
    }
}

// TODO (cleanup) Return Result.
pub fn load<T: DeserializeOwned>(file_path: &PathBuf) -> T {
    let binary = std::fs::read(file_path).expect(&*format!("load from {}", &file_path.display()));
    let mut decoder = GzDecoder::new(binary.as_slice());
    let mut binary = vec![];
    decoder.read_to_end(&mut binary).expect("unzip serialized");
    rmp_serde::from_slice(&binary).expect("deserialize")
}

// TODO (cleanup) Return a Result.
pub fn store<T: Serialize>(x: &T, file_path: &PathBuf, compact: bool) {
    // TODO (improvement) When using compact representation (without field names),
    //   add some format version info in the data and/or in file names. Consider using protobuf.
    let mut binary = Vec::new();
    let mut serializer = rmp_serde::Serializer::new(&mut binary);
    if compact {
        x.serialize(&mut serializer)
    } else {
        x.serialize(&mut serializer.with_struct_map())
    }
    .expect("serialize");

    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&binary.as_slice())
        .expect("gzip serialized");
    let binary = encoder.finish().expect("gzip serialized");
    std::fs::write(file_path, &binary).expect(&*format!("write to {}", &file_path.display()));
}

#[cfg(test)]
mod tests {}
