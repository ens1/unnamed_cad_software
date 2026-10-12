//! Small process-local proofs for unchanged, successfully decoded default maps.
//! Package readers still verify resource hashes and exact bytes before this
//! adapter is called. Proofs contain neither decoded pixels nor renderer state.
use image::ImageFormat;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Mutex, OnceLock},
};

const MAX_PROOFS: usize = 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Codec {
    Png,
    Jpeg,
    Tiff,
    WebP,
}
type Key = ([u8; 32], Codec);
#[derive(Default)]
struct Proofs {
    successful: BTreeSet<Key>,
    order: VecDeque<Key>,
}
impl Proofs {
    fn insert(&mut self, key: Key) {
        if !self.successful.insert(key) {
            return;
        }
        self.order.push_back(key);
        while self.order.len() > MAX_PROOFS {
            if let Some(old) = self.order.pop_front() {
                self.successful.remove(&old);
            }
        }
    }
}
static PROOFS: OnceLock<Mutex<Proofs>> = OnceLock::new();

pub(super) fn validate(bytes: &[u8], kind: &str) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() as u64 > super::MAX_ENCODED_BYTES {
        return Err("Default map exceeds the nonempty 64 MiB encoded image bound".into());
    }
    // Normalize only supported codec aliases. A PNG proof can never authorize
    // those same bytes labelled as JPEG, TIFF or WebP.
    let codec = match super::format(kind)? {
        ImageFormat::Png => Codec::Png,
        ImageFormat::Jpeg => Codec::Jpeg,
        ImageFormat::Tiff => Codec::Tiff,
        ImageFormat::WebP => Codec::WebP,
        _ => return Err("Unsupported default map codec".into()),
    };
    // Rehash the actual bounded slice on every call. Paths, declared hashes and
    // file timestamps cannot replace this proof of identical source bytes.
    let key = (Sha256::digest(bytes).into(), codec);
    let proofs = PROOFS.get_or_init(|| Mutex::new(Proofs::default()));
    if let Ok(proofs) = proofs.lock()
        && proofs.successful.contains(&key)
    {
        return Ok(());
    }
    // The codec/header, dimension, allocation and complete decode checks remain
    // the same as uncached validation. Do not hold the cache lock while decoding.
    super::decode(bytes, kind)?;
    if let Ok(mut proofs) = proofs.lock() {
        proofs.insert(key);
    }
    Ok(())
}
