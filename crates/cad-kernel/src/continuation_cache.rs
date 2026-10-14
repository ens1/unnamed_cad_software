//! Bounded process-local evidence cache. No proof data enters document source.
use monstertruck::geometry::prelude::SurfaceParameterTube;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::VecDeque, io::Write};

type Key = [u8; 32];
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 128;

#[derive(Default)]
struct Cache {
    entries: VecDeque<(Key, Box<[SurfaceParameterTube]>)>,
    bytes: usize,
}
thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// Hash the complete current geometry and residual without allocating its JSON.
/// Oversized or unencodable sources simply bypass the optional evidence cache.
pub(crate) fn key(source: &impl serde::Serialize, linear_mm: f64) -> Option<Key> {
    struct Hasher {
        digest: Sha256,
        remaining: usize,
    }
    impl Write for Hasher {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.remaining {
                return Err(std::io::Error::other("Continuation cache key budget"));
            }
            self.remaining -= bytes.len();
            self.digest.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut hasher = Hasher {
        digest: Sha256::new(),
        remaining: 1024 * 1024,
    };
    hasher.digest.update(b"tcad-native-continuation/1");
    hasher.digest.update(linear_mm.to_bits().to_le_bytes());
    serde_json::to_writer(&mut hasher, source).ok()?;
    Some(hasher.digest.finalize().into())
}

pub(crate) fn get(key: Key) -> Option<Vec<SurfaceParameterTube>> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let index = cache
            .entries
            .iter()
            .position(|(candidate, _)| *candidate == key)?;
        let entry = cache.entries.remove(index)?;
        let result = entry.1.to_vec();
        cache.entries.push_back(entry);
        Some(result)
    })
}

pub(crate) fn insert(key: Key, tubes: &[SurfaceParameterTube]) {
    let bytes = std::mem::size_of_val(tubes);
    if bytes > MAX_BYTES {
        return;
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(index) = cache
            .entries
            .iter()
            .position(|(candidate, _)| *candidate == key)
            && let Some((_, old)) = cache.entries.remove(index)
        {
            cache.bytes -= std::mem::size_of_val(old.as_ref());
        }
        while cache.bytes + bytes > MAX_BYTES || cache.entries.len() >= MAX_ENTRIES {
            let Some((_, old)) = cache.entries.pop_front() else {
                break;
            };
            cache.bytes -= std::mem::size_of_val(old.as_ref());
        }
        cache.bytes += bytes;
        cache.entries.push_back((key, tubes.into()));
    });
}
