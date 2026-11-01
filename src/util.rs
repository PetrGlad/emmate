use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::Serialize;
use serde::de::DeserializeOwned;

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

// TODO (cleanup) Return a Result instead of panicking.
pub fn load<T: DeserializeOwned>(file_path: &PathBuf) -> T {
    let binary = std::fs::read(file_path).expect(&*format!("load from {}", &file_path.display()));
    decode(&binary).expect("deserialize")
}

// TODO (cleanup) Return a Result instead of panicking.
pub fn store<T: Serialize>(x: &T, file_path: &PathBuf, compact: bool) {
    let binary = encode(x, compact);
    std::fs::write(&file_path, &binary).expect(&*format!("write to {}", &file_path.display()));
}

/// Serialize and compress the value.
pub fn encode<T: Serialize>(x: &T, compact: bool) -> Vec<u8> {
    // TODO (improvement) When using compact representation (without field names),
    //   add some format version info in the data and/or in file names anyway.
    //   Consider using protobuf.
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
    encoder.finish().expect("gzip serialized")
}

/// Reverse of `encode`.
pub fn decode<T: DeserializeOwned>(data: &[u8]) -> io::Result<T> {
    let mut decoder = GzDecoder::new(data);
    let mut binary = vec![];
    decoder.read_to_end(&mut binary)?;
    rmp_serde::from_slice(&binary).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write the data to a temporary file then rename it to the target name,
/// so readers never see a partially written file.
pub fn write_atomic(file_path: &Path, data: &[u8]) -> io::Result<()> {
    let mut temp_name = file_path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_owned();
    // Process id prevents clashes between application instances writing the same file.
    temp_name.push(format!(".{}.tmp", std::process::id()));
    let temp_path = file_path.with_file_name(temp_name);
    let result = std::fs::write(&temp_path, data).and_then(|_| std::fs::rename(&temp_path, file_path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let x = vec![(1u8, "a".to_string()), (2, "b".to_string())];
        assert_eq!(x, decode::<Vec<(u8, String)>>(&encode(&x, false)).unwrap());
        assert_eq!(x, decode::<Vec<(u8, String)>>(&encode(&x, true)).unwrap());
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode::<Vec<u8>>(b"not gzip").is_err());
    }

    #[test]
    fn write_atomic_replaces_file_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("emmate-util-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("data");
        write_atomic(&target, b"one").unwrap();
        write_atomic(&target, b"two").unwrap();
        assert_eq!(b"two".to_vec(), std::fs::read(&target).unwrap());
        assert_eq!(1, std::fs::read_dir(&dir).unwrap().count());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
