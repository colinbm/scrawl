//! The `.scrawl` file format: a zip archive holding `meta.json`, `document.json`, and the images
//! the document uses under `images/`, named by asset id.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::Document;

pub const FILE_EXTENSION: &str = "scrawl";
pub const FORMAT_VERSION: u32 = 1;

const META: &str = "meta.json";
const DOCUMENT: &str = "document.json";
const IMAGES: &str = "images/";

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a valid Scrawl file: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("corrupt document: {0}")]
    Json(#[from] serde_json::Error),
    #[error("this file was made by a newer version of Scrawl (format {0}, supported up to {FORMAT_VERSION})")]
    TooNew(u32),
}

#[derive(Serialize, Deserialize)]
struct Meta {
    format_version: u32,
    app_version: String,
}

pub fn to_bytes(doc: &Document) -> Result<Vec<u8>, FileError> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let meta = Meta { format_version: FORMAT_VERSION, app_version: env!("CARGO_PKG_VERSION").into() };
    zip.start_file(META, opts)?;
    zip.write_all(&serde_json::to_vec_pretty(&meta)?)?;
    zip.start_file(DOCUMENT, opts)?;
    zip.write_all(&serde_json::to_vec(doc)?)?;
    // Only what's in use: undo may keep others alive in memory, but they needn't be saved.
    // Images are already compressed.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for id in doc.used_assets() {
        if let Some(bytes) = doc.assets.get(&id) {
            zip.start_file(format!("{IMAGES}{id}"), stored)?;
            zip.write_all(bytes)?;
        }
    }
    Ok(zip.finish()?.into_inner())
}

pub fn from_bytes(bytes: &[u8]) -> Result<Document, FileError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    let meta: Meta = serde_json::from_slice(&read_entry(&mut zip, META)?)?;
    if meta.format_version > FORMAT_VERSION {
        return Err(FileError::TooNew(meta.format_version));
    }
    let mut doc: Document = serde_json::from_slice(&read_entry(&mut zip, DOCUMENT)?)?;
    let images: Vec<String> = zip
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.into_owned()))
        .filter(|n| n.starts_with(IMAGES) && n.len() > IMAGES.len())
        .collect();
    for name in images {
        let bytes = read_entry(&mut zip, &name)?;
        doc.assets.insert(name[IMAGES.len()..].to_owned(), bytes.into());
    }
    // Instance contents aren't saved. Rebuild them so the document renders as-is; text in them is
    // sized once a text engine refits the document.
    crate::symbols::sync_instances(&mut doc, &mut |_| {});
    Ok(doc)
}

fn read_entry(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, FileError> {
    let mut buf = vec![];
    zip.by_name(name)?.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn save(doc: &Document, path: &Path) -> Result<(), FileError> {
    // Write to a sibling temp file then rename, so a failed save never clobbers the original.
    let tmp = path.with_extension(format!("{FILE_EXTENSION}.tmp"));
    std::fs::write(&tmp, to_bytes(doc)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Document, FileError> {
    from_bytes(&std::fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, Layer, ops};
    use kurbo::Point;

    #[test]
    fn round_trip_every_layer_kind() {
        let mut doc = Document::default();
        let page = &mut doc.pages[0];
        ops::insert_drawn(page, Layer::artboard("A", Frame::new(0.0, 0.0, 300.0, 300.0)));
        let a = ops::insert_drawn(page, Layer::rectangle(Frame::new(10.0, 10.0, 50.0, 50.0)));
        let b = ops::insert_drawn(page, Layer::oval(Frame::new(80.0, 10.0, 50.0, 50.0)));
        ops::insert_drawn(page, Layer::line(Point::new(0.0, 100.0), Point::new(100.0, 150.0)));
        ops::insert_drawn(page, Layer::text(Point::new(5.0, 200.0), "Hello"));
        ops::group(page, &[a, b]);
        page.find_mut(a).unwrap().rotation = 12.5;

        let back = from_bytes(&to_bytes(&doc).unwrap()).unwrap();
        assert_eq!(back, doc);
    }

    #[test]
    fn images_are_stored_beside_the_document() {
        let mut doc = Document::default();
        let png = [0x89, b'P', b'N', b'G', 0, 1, 2, 3].to_vec();
        let id = doc.assets.add_image(png.clone()).unwrap();
        doc.assets.add_image([0xFF, 0xD8, 0xFF, 0].to_vec());
        doc.pages[0].layers.push(Layer::image(id.clone(), Frame::new(0.0, 0.0, 10.0, 10.0)));
        let back = from_bytes(&to_bytes(&doc).unwrap()).unwrap();
        assert_eq!(back.assets.len(), 1, "unused images aren't saved");
        assert_eq!(&back.assets.get(&id).unwrap()[..], &png[..]);
        assert_eq!(back.pages, doc.pages);
    }

    #[test]
    fn sample_document_round_trips() {
        let doc = crate::sample::sample_document();
        assert_eq!(from_bytes(&to_bytes(&doc).unwrap()).unwrap(), doc);
    }

    #[test]
    fn rejects_newer_format() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(META, SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"{"format_version": 999, "app_version": "9"}"#).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(matches!(from_bytes(&bytes), Err(FileError::TooNew(999))));
    }
}
