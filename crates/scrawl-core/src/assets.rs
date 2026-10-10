//! Binary assets (images) a document uses, stored beside the JSON in `.scrawl` files.
//!
//! Assets are content-addressed: an id is a hash of the bytes plus a file extension, so equal ids
//! mean equal content. That keeps comparing documents (for undo and unsaved-change checks) cheap,
//! and cloning is cheap too since the bytes are shared.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

/// Encoded image files by asset id, e.g. `"3fa2…c1.png"`.
#[derive(Clone, Debug, Default)]
pub struct Assets(BTreeMap<String, Arc<[u8]>>);

impl PartialEq for Assets {
    fn eq(&self, other: &Self) -> bool {
        // Ids are content hashes, so equal keys mean equal contents.
        self.0.keys().eq(other.0.keys())
    }
}

/// The file extension for an encoded image, sniffed from its first bytes.
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        _ => None,
    }
}

/// A content-derived asset id for `bytes`.
pub fn asset_id(bytes: &[u8], extension: &str) -> String {
    let half = |seed: u64| {
        let mut h = DefaultHasher::new();
        seed.hash(&mut h);
        bytes.hash(&mut h);
        h.finish()
    };
    format!("{:016x}{:016x}.{extension}", half(0x005c_7a11), half(0x00a5_5e75))
}

impl Assets {
    /// Stores an encoded image, returning its id; `None` if it isn't a supported format.
    pub fn add_image(&mut self, bytes: Vec<u8>) -> Option<String> {
        let ext = image_extension(&bytes)?;
        let id = asset_id(&bytes, ext);
        self.0.entry(id.clone()).or_insert_with(|| bytes.into());
        Some(id)
    }

    /// Stores bytes under a known id, as read from a file.
    pub fn insert(&mut self, id: String, bytes: Arc<[u8]>) {
        self.0.insert(id, bytes);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<[u8]>> {
        self.0.get(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Arc<[u8]>)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_content_based() {
        let png = [0x89, b'P', b'N', b'G', 1, 2, 3].to_vec();
        let mut a = Assets::default();
        let id = a.add_image(png.clone()).unwrap();
        assert!(id.ends_with(".png"));
        assert_eq!(a.add_image(png).unwrap(), id);
        assert_eq!(a.len(), 1);
        assert!(a.add_image(b"not an image".to_vec()).is_none());
        let mut b = Assets::default();
        b.add_image([0xFF, 0xD8, 0xFF, 9].to_vec());
        assert_ne!(a, b);
    }
}
