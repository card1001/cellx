//! Pictures placed over the sheet (삽입 → 그림), and the pictures of xlsx files,
//! which are kept when a file is opened and saved again.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{CellxError, Result};
use crate::extras::Anchor;
use crate::ooxml::{self, PictureXml, EMU_PER_PX};
use crate::workbook::Workbook;

/// Image bytes, shared: the undo history keeps copies of the object list, and
/// those must not copy the images. Two blobs are equal when they are the same
/// bytes in memory.
#[derive(Clone, Default)]
pub struct Blob(pub Arc<Vec<u8>>);

impl PartialEq for Blob {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Blob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Blob({} bytes)", self.0.len())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Picture {
    pub id: u32,
    pub sheet: u32,
    /// Shown in the name box and kept in xlsx, e.g. "그림 1".
    pub name: String,
    /// image/png, image/jpeg, image/gif or image/bmp.
    pub mime: String,
    pub anchor: Anchor,
    /// Width ÷ height of the image itself, for resizing with the aspect kept.
    pub ratio: f64,
    #[serde(skip)]
    pub data: Blob,
}

/// What the bytes are and how big the image is, in pixels, from its header.
pub fn image_info(bytes: &[u8]) -> Option<(&'static str, &'static str, u32, u32)> {
    let be16 = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as u32);
    let le16 = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u32);
    let be32 = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let le32 = |i: usize| bytes.get(i..i + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]).unsigned_abs());
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(("image/png", "png", be32(16)?, be32(20)?));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(("image/gif", "gif", le16(6)?, le16(8)?));
    }
    if bytes.starts_with(b"BM") {
        return Some(("image/bmp", "bmp", le32(18)?, le32(22)?));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // Walk the segments to the frame header (SOF0..SOF15 but DHT, JPG, DAC).
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) || marker == 0xFF {
                i += if marker == 0xFF { 1 } else { 2 };
                continue;
            }
            let len = be16(i + 2)? as usize;
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                return Some(("image/jpeg", "jpeg", be16(i + 7)?, be16(i + 5)?));
            }
            i += 2 + len;
        }
        return Some(("image/jpeg", "jpeg", 0, 0));
    }
    None
}

/// The file extension xlsx uses for a picture's media part.
pub(crate) fn extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        _ => "jpeg",
    }
}

impl Workbook {
    pub fn pictures(&self, sheet: u32) -> Vec<Picture> {
        self.pictures.iter().filter(|p| p.sheet == sheet).cloned().collect()
    }

    pub fn picture_bytes(&self, id: u32) -> Result<(String, Arc<Vec<u8>>)> {
        let p = self
            .pictures
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| CellxError::Calc("그림이 없습니다".into()))?;
        Ok((p.mime.clone(), p.data.0.clone()))
    }

    /// The image as a `data:` URL, for the screen.
    pub fn picture_data_url(&self, id: u32) -> Result<String> {
        use base64::Engine;
        let (mime, bytes) = self.picture_bytes(id)?;
        Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes.as_slice())))
    }

    /// A pasted image (clipboard), given as base64 or a `data:` URL.
    pub fn add_picture_base64(&mut self, sheet: u32, row: i32, col: i32, data: &str) -> Result<Picture> {
        use base64::Engine;
        let b64 = data.split_once(";base64,").map_or(data, |(_, b)| b);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|_| CellxError::Calc("그림 데이터를 읽을 수 없습니다.".into()))?;
        self.add_picture(sheet, row, col, bytes)
    }

    /// 삽입 → 그림: puts the image at the top-left corner of cell (`row`, `col`),
    /// at its own size but no wider than 640 px or taller than 480 px, as Excel does
    /// with a large picture.
    pub fn add_picture(&mut self, sheet: u32, row: i32, col: i32, bytes: Vec<u8>) -> Result<Picture> {
        self.guard(sheet, crate::workbook::CellRange { r0: row, c0: col, r1: row, c1: col }, crate::sheetopts::Guard::Objects)?;
        let (mime, _, w, h) = image_info(&bytes)
            .ok_or_else(|| CellxError::Calc("PNG, JPEG, GIF, BMP 그림만 넣을 수 있습니다.".into()))?;
        let (w, h) = if w == 0 || h == 0 { (320.0, 240.0) } else { (w as f64, h as f64) };
        let scale = (640.0 / w).min(480.0 / h).min(1.0);
        let number = self.pictures.iter().filter(|p| p.sheet == sheet).count() + 1;
        let id = self.next_object_id;
        self.next_object_id += 1;
        let picture = Picture {
            id,
            sheet,
            name: format!("그림 {number}"),
            mime: mime.to_string(),
            anchor: Anchor { row, col, dx: 0.0, dy: 0.0, width: (w * scale).round(), height: (h * scale).round() },
            ratio: w / h,
            data: Blob(Arc::new(bytes)),
        };
        let start = self.begin();
        self.pictures.push(picture.clone());
        self.commit(start)?;
        Ok(picture)
    }

    /// Moves, resizes or renames a picture (its image stays).
    pub fn update_picture(&mut self, id: u32, name: String, anchor: Anchor) -> Result<()> {
        let sheet = self
            .pictures
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.sheet)
            .ok_or_else(|| CellxError::Calc("그림이 없습니다".into()))?;
        let at = crate::workbook::CellRange { r0: anchor.row, c0: anchor.col, r1: anchor.row, c1: anchor.col };
        self.guard(sheet, at, crate::sheetopts::Guard::Objects)?;
        let start = self.begin();
        if let Some(p) = self.pictures.iter_mut().find(|p| p.id == id) {
            p.name = name;
            p.anchor = Anchor {
                width: anchor.width.max(4.0),
                height: anchor.height.max(4.0),
                row: anchor.row.max(1),
                col: anchor.col.max(1),
                ..anchor
            };
        }
        self.commit(start)?;
        Ok(())
    }

    pub fn delete_picture(&mut self, id: u32) -> Result<()> {
        if let Some(p) = self.pictures.iter().find(|p| p.id == id) {
            let at = crate::workbook::CellRange { r0: p.anchor.row, c0: p.anchor.col, r1: p.anchor.row, c1: p.anchor.col };
            self.guard(p.sheet, at, crate::sheetopts::Guard::Objects)?;
        }
        let start = self.begin();
        self.pictures.retain(|p| p.id != id);
        self.commit(start)?;
        Ok(())
    }

    /// Copies a picture next to itself (Ctrl+D on a picture, or copy and paste).
    pub fn duplicate_picture(&mut self, id: u32, sheet: u32, anchor: Anchor) -> Result<Picture> {
        let mut p = self
            .pictures
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| CellxError::Calc("그림이 없습니다".into()))?;
        p.id = self.next_object_id;
        self.next_object_id += 1;
        p.sheet = sheet;
        p.anchor = anchor;
        p.name = format!("그림 {}", self.pictures.iter().filter(|q| q.sheet == sheet).count() + 1);
        let start = self.begin();
        self.pictures.push(p.clone());
        self.commit(start)?;
        Ok(p)
    }

    pub(crate) fn picture_parts(&self) -> Vec<(u32, PictureXml)> {
        self.pictures
            .iter()
            .map(|p| {
                let (from, to) = self.to_marker(p.sheet, &p.anchor);
                (
                    p.sheet,
                    PictureXml {
                        name: p.name.clone(),
                        ext: extension(&p.mime),
                        data: p.data.0.clone(),
                        from,
                        to,
                        cx: (p.anchor.width * EMU_PER_PX) as i64,
                        cy: (p.anchor.height * EMU_PER_PX) as i64,
                    },
                )
            })
            .collect()
    }

    /// Pictures of an xlsx file.
    pub(crate) fn import_pictures(&mut self, xlsx: &[u8]) {
        for ip in ooxml::read_pictures(xlsx) {
            let Some((mime, _, w, h)) = image_info(&ip.data) else { continue };
            let sheet = ip.sheet as u32;
            let dx = ip.from.col_off as f64 / EMU_PER_PX;
            let dy = ip.from.row_off as f64 / EMU_PER_PX;
            let (width, height) = match (ip.ext, ip.to) {
                (Some((cx, cy)), _) if cx > 0 && cy > 0 => (cx as f64 / EMU_PER_PX, cy as f64 / EMU_PER_PX),
                (_, Some(to)) => {
                    let w: f64 = ((ip.from.col + 1)..=to.col).map(|c| self.col_px(sheet, c)).sum::<f64>() - dx
                        + to.col_off as f64 / EMU_PER_PX;
                    let h: f64 = ((ip.from.row + 1)..=to.row).map(|r| self.row_px(sheet, r)).sum::<f64>() - dy
                        + to.row_off as f64 / EMU_PER_PX;
                    (w, h)
                }
                _ => (w.max(1) as f64, h.max(1) as f64),
            };
            let id = self.next_object_id;
            self.next_object_id += 1;
            let number = self.pictures.iter().filter(|p| p.sheet == sheet).count() + 1;
            self.pictures.push(Picture {
                id,
                sheet,
                name: if ip.name.is_empty() { format!("그림 {number}") } else { ip.name },
                mime: mime.to_string(),
                anchor: Anchor { row: ip.from.row + 1, col: ip.from.col + 1, dx, dy, width: width.max(4.0), height: height.max(4.0) },
                ratio: if w > 0 && h > 0 { w as f64 / h as f64 } else { width / height.max(1.0) },
                data: Blob(Arc::new(ip.data)),
            });
        }
    }
}

/// A 3 × 2 PNG header, for tests elsewhere.
#[cfg(test)]
pub(crate) fn tests_png() -> Vec<u8> {
    tests::png()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3 × 2 red PNG.
    pub(crate) fn png() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        v.extend_from_slice(&3u32.to_be_bytes());
        v.extend_from_slice(&2u32.to_be_bytes());
        v.extend_from_slice(b"\x08\x02\0\0\0\0\0\0\0");
        v
    }

    #[test]
    fn image_headers() {
        assert_eq!(image_info(&png()), Some(("image/png", "png", 3, 2)));
        let gif = b"GIF89a\x0a\x00\x05\x00".to_vec();
        assert_eq!(image_info(&gif), Some(("image/gif", "gif", 10, 5)));
        // SOI, APP0 (len 4), SOF0 with height 7 and width 9.
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x07, 0x00, 0x09, 0x03, 0, 0, 0, 0];
        assert_eq!(image_info(&jpg), Some(("image/jpeg", "jpeg", 9, 7)));
        assert_eq!(image_info(b"not an image"), None);
    }

    #[test]
    fn pictures_round_trip_through_xlsx_and_follow_rows() {
        let mut wb = Workbook::new().unwrap();
        wb.set_input(0, 1, 1, "x").unwrap();
        let p = wb.add_picture(0, 3, 2, png()).unwrap();
        assert_eq!((p.anchor.width, p.anchor.height, p.name.as_str()), (3.0, 2.0, "그림 1"));
        wb.update_picture(p.id, "로고".into(), Anchor { width: 120.0, height: 80.0, ..p.anchor.clone() }).unwrap();
        wb.insert_rows(0, 1, 2).unwrap();
        assert_eq!(wb.pictures(0)[0].anchor.row, 5);
        wb.undo().unwrap();
        assert_eq!(wb.pictures(0)[0].anchor.row, 3);

        let bytes = wb.xlsx_bytes().unwrap();
        let dir = std::env::temp_dir().join(format!("cellx-pic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.xlsx");
        std::fs::write(&path, &bytes).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let pics = back.pictures(0);
        assert_eq!(pics.len(), 1);
        assert_eq!((pics[0].name.as_str(), pics[0].anchor.row, pics[0].anchor.col), ("로고", 3, 2));
        assert!((pics[0].anchor.width - 120.0).abs() < 1.0 && (pics[0].anchor.height - 80.0).abs() < 1.0);
        assert_eq!(back.picture_bytes(pics[0].id).unwrap().1.as_slice(), png().as_slice());
        // And once more: a file with a picture saves its picture again.
        let again = back.xlsx_bytes().unwrap();
        std::fs::write(&path, &again).unwrap();
        assert_eq!(Workbook::open_xlsx(&path).unwrap().pictures(0).len(), 1);
        std::fs::remove_dir_all(&dir).ok();

        wb.delete_picture(p.id).unwrap();
        assert!(wb.pictures(0).is_empty());
        wb.undo().unwrap();
        assert_eq!(wb.pictures(0).len(), 1);
    }
}

/// `cargo test -p cellx-core write_picture_sample -- --ignored`: an xlsx with a
/// picture, from `CELLX_PICTURE` (a PNG/JPEG file), for checking by eye.
#[cfg(test)]
#[test]
#[ignore]
fn write_picture_sample() {
    let src = std::env::var("CELLX_PICTURE").unwrap_or_else(|_| "/tmp/pic.png".into());
    let mut wb = Workbook::new().unwrap();
    wb.set_inputs(0, &[(1, 1, "그림 예제".into()), (2, 1, "아래 그림은 xlsx에 들어 있습니다".into())]).unwrap();
    wb.add_picture(0, 4, 2, std::fs::read(src).unwrap()).unwrap();
    std::fs::write("/tmp/picture-sample.xlsx", wb.xlsx_bytes().unwrap()).unwrap();
}
