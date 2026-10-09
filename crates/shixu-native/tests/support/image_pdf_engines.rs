//! Explicit synthetic-only native harness. Never linked into the library/worker.
use image::{DynamicImage, ImageFormat, RgbImage};
use libloading::Library;
use serde_json::Value;
use sha2::{Digest, Sha256};
use shixu_core::contracts::{AppResult, error::AppError};
use shixu_native::attachments::{
    image::{OcrEngine, Word},
    pdf::{PdfDocument, TextRegion},
};
use std::{
    ffi::c_void,
    io::{Cursor, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
pub static NATIVE_LOCK: Mutex<()> = Mutex::new(());
const ENGINE: &str = include_str!("../../../../tests/fixtures/attachments/engine_manifest.json");
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/attachments")
}
pub fn manifest(kind: &str) -> Value {
    serde_json::from_str(match kind {
        "image" => include_str!("../../../../tests/fixtures/attachments/image_manifest.json"),
        "pdf" => include_str!("../../../../tests/fixtures/attachments/pdf_manifest.json"),
        _ => panic!("unknown fixture kind"),
    })
    .unwrap()
}
pub fn approved_bytes(kind: &str, name: &str) -> Vec<u8> {
    let m = manifest(kind);
    let record = m["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["file"] == name)
        .expect("non-allowlisted fixture refused");
    assert!(!name.contains(['/', '\\']));
    let path = root().join(name);
    assert!(!path.symlink_metadata().unwrap().file_type().is_symlink());
    let data = std::fs::read(path).unwrap();
    assert_eq!(
        Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        record["sha256"].as_str().unwrap(),
        "fixture hash mismatch"
    );
    data
}
pub fn verify_engines() {
    let m: Value = serde_json::from_str(ENGINE).unwrap();
    for file in m["files"].as_array().unwrap() {
        let data = std::fs::read(file["path"].as_str().unwrap())
            .expect("BLOCKED: missing pinned N4 engine/model");
        assert_eq!(
            Sha256::digest(&data)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            file["sha256"].as_str().unwrap(),
            "BLOCKED: engine/model hash mismatch"
        );
    }
}
pub struct Tesseract;
impl OcrEngine for Tesseract {
    fn version(&self) -> &str {
        "tesseract/5.5.0;tessdata_fast/87416418657359cb625c412a48b6e1d6d41c29bd"
    }
    fn recognize(&mut self, pixels: &RgbImage) -> AppResult<Vec<Word>> {
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(pixels.clone())
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|_| AppError::ParseFailed)?;
        let mut child = Command::new("/usr/bin/tesseract")
            .args([
                "stdin",
                "stdout",
                "--tessdata-dir",
                "/workspace/.toolchains/tessdata",
                "-l",
                "chi_sim+eng",
                "--psm",
                "6",
                "-c",
                "tessedit_create_tsv=1",
            ])
            .env("OMP_THREAD_LIMIT", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| AppError::Unsupported)?;
        let mut input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let bytes = std::thread::scope(|s| -> AppResult<Vec<u8>> {
            let writer = s.spawn(move || input.write_all(png.get_ref()));
            let reader = s.spawn(move || {
                let mut bytes = vec![];
                output
                    .take(8 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map(|_| bytes)
            });
            let start = Instant::now();
            loop {
                if let Some(status) = child.try_wait().map_err(|_| AppError::ParseFailed)? {
                    if !status.success() {
                        return Err(AppError::ParseFailed);
                    }
                    break;
                }
                if start.elapsed() > Duration::from_secs(30) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::ParseFailed);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            writer.join().unwrap().map_err(|_| AppError::ParseFailed)?;
            reader.join().unwrap().map_err(|_| AppError::ParseFailed)
        })?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(AppError::ParseFailed);
        }
        let tsv = std::str::from_utf8(&bytes).map_err(|_| AppError::ParseFailed)?;
        let mut words = vec![];
        for line in tsv.lines().skip(1) {
            let fields: Vec<_> = line.splitn(12, '\t').collect();
            if fields.len() != 12 || fields[0] != "5" || fields[11].trim().is_empty() {
                continue;
            }
            let n = |i: usize| fields[i].parse::<u32>().map_err(|_| AppError::ParseFailed);
            words.push(Word {
                text: fields[11].into(),
                x: n(6)?,
                y: n(7)?,
                width: n(8)?,
                height: n(9)?,
                confidence: fields[10].parse().map_err(|_| AppError::ParseFailed)?,
            });
        }
        Ok(words)
    }
}
type Handle = *mut c_void;
// Every symbol's signature is taken from PDFium public headers in the pinned
// wheel. Library/document/page/bitmap lifetimes stay on the serialized test thread.
macro_rules! call {
    ($lib:expr,$name:literal,$ty:ty,$($arg:expr),* $(,)?) => {{
        let f:libloading::Symbol<$ty>=$lib.get(concat!($name,"\0").as_bytes()).expect("PDFium ABI symbol");f($($arg),*)
    }};
}
pub struct Pdfium {
    lib: Library,
}
impl Pdfium {
    pub fn new() -> Self {
        verify_engines();
        let m: Value = serde_json::from_str(ENGINE).unwrap();
        let path = m["files"].as_array().unwrap().last().unwrap()["path"]
            .as_str()
            .unwrap();
        unsafe {
            let lib = Library::new(path).unwrap();
            call!(lib, "FPDF_InitLibrary", unsafe extern "C" fn(),);
            Self { lib }
        }
    }
    pub fn open<'a>(&'a self, bytes: Vec<u8>) -> AppResult<Document<'a>> {
        if bytes.len() > 20 * 1024 * 1024 {
            return Err(AppError::InvalidInput);
        }
        unsafe {
            let h = call!(
                self.lib,
                "FPDF_LoadMemDocument64",
                unsafe extern "C" fn(*const c_void, usize, *const i8) -> Handle,
                bytes.as_ptr().cast(),
                bytes.len(),
                std::ptr::null()
            );
            if h.is_null() {
                let e = call!(self.lib, "FPDF_GetLastError", unsafe extern "C" fn() -> u64,);
                return Err(if e == 4 {
                    AppError::AuthFailed
                } else {
                    AppError::ParseFailed
                });
            }
            Ok(Document {
                engine: self,
                h,
                _bytes: bytes,
            })
        }
    }
}
impl Drop for Pdfium {
    fn drop(&mut self) {
        unsafe {
            call!(self.lib, "FPDF_DestroyLibrary", unsafe extern "C" fn(),);
        }
    }
}
pub struct Document<'a> {
    engine: &'a Pdfium,
    h: Handle,
    _bytes: Vec<u8>,
}
impl Drop for Document<'_> {
    fn drop(&mut self) {
        unsafe {
            call!(
                self.engine.lib,
                "FPDF_CloseDocument",
                unsafe extern "C" fn(Handle),
                self.h
            );
        }
    }
}
struct Page<'a> {
    doc: &'a Document<'a>,
    h: Handle,
}
impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe {
            call!(
                self.doc.engine.lib,
                "FPDF_ClosePage",
                unsafe extern "C" fn(Handle),
                self.h
            );
        }
    }
}
impl Document<'_> {
    fn page(&self, p: u32) -> AppResult<Page<'_>> {
        unsafe {
            let h = call!(
                self.engine.lib,
                "FPDF_LoadPage",
                unsafe extern "C" fn(Handle, i32) -> Handle,
                self.h,
                p as i32
            );
            if h.is_null() {
                Err(AppError::ParseFailed)
            } else {
                Ok(Page { doc: self, h })
            }
        }
    }
}
impl PdfDocument for Document<'_> {
    fn version(&self) -> &str {
        "pdfium/145.0.7616.0;v8=false;xfa=false"
    }
    fn page_count(&self) -> u32 {
        unsafe {
            call!(
                self.engine.lib,
                "FPDF_GetPageCount",
                unsafe extern "C" fn(Handle) -> i32,
                self.h
            )
            .max(0) as u32
        }
    }
    fn page_size(&self, page: u32) -> AppResult<(u32, u32)> {
        let (mut w, mut h) = (0f64, 0f64);
        let ok = unsafe {
            call!(
                self.engine.lib,
                "FPDF_GetPageSizeByIndex",
                unsafe extern "C" fn(Handle, i32, *mut f64, *mut f64) -> i32,
                self.h,
                page as i32,
                &mut w,
                &mut h
            )
        };
        if ok == 0
            || !w.is_finite()
            || !h.is_finite()
            || w <= 0.
            || h <= 0.
            || w > u32::MAX as f64
            || h > u32::MAX as f64
        {
            return Err(AppError::ParseFailed);
        }
        Ok((w.ceil() as u32, h.ceil() as u32))
    }
    fn native_text(&self, page: u32, max_chars: u32) -> AppResult<Vec<TextRegion>> {
        let (_, height) = self.page_size(page)?;
        let p = self.page(page)?;
        unsafe {
            let lib = &self.engine.lib;
            let tp = call!(
                lib,
                "FPDFText_LoadPage",
                unsafe extern "C" fn(Handle) -> Handle,
                p.h
            );
            if tp.is_null() {
                return Err(AppError::ParseFailed);
            }
            let result = (|| {
                let n = call!(
                    lib,
                    "FPDFText_CountChars",
                    unsafe extern "C" fn(Handle) -> i32,
                    tp
                );
                if n < 0 || n as u32 > max_chars {
                    return Err(AppError::InvalidInput);
                }
                let mut regions = vec![];
                let mut space_before = false;
                for i in 0..n {
                    let code = call!(
                        lib,
                        "FPDFText_GetUnicode",
                        unsafe extern "C" fn(Handle, i32) -> u32,
                        tp,
                        i
                    );
                    let ch = char::from_u32(code).ok_or(AppError::ParseFailed)?;
                    if ch.is_whitespace() {
                        space_before = true;
                        continue;
                    }
                    let (mut left, mut right, mut bottom, mut top) = (0., 0., 0., 0.);
                    let ok = call!(
                        lib,
                        "FPDFText_GetCharBox",
                        unsafe extern "C" fn(
                            Handle,
                            i32,
                            *mut f64,
                            *mut f64,
                            *mut f64,
                            *mut f64,
                        ) -> i32,
                        tp,
                        i,
                        &mut left,
                        &mut right,
                        &mut bottom,
                        &mut top
                    );
                    if ok == 0
                        || [left, right, bottom, top].iter().any(|v| !v.is_finite())
                        || left < 0.
                        || bottom < 0.
                        || top > height as f64
                        || right <= left
                        || top <= bottom
                    {
                        return Err(AppError::ParseFailed);
                    }
                    let x = left.floor() as u32;
                    let y = (height as f64 - top).floor() as u32;
                    let text = if space_before {
                        format!(" {ch}")
                    } else {
                        ch.to_string()
                    };
                    space_before = false;
                    regions.push(TextRegion {
                        text,
                        x,
                        y,
                        width: right.ceil() as u32 - x,
                        height: (height as f64 - bottom).ceil() as u32 - y,
                    });
                }
                Ok(regions)
            })();
            call!(lib, "FPDFText_ClosePage", unsafe extern "C" fn(Handle), tp);
            result
        }
    }
    fn render(&self, page: u32, width: u32, height: u32) -> AppResult<RgbImage> {
        // Independent allocation cap even if portable preflight regresses.
        if width == 0
            || height == 0
            || width > 10000
            || height > 10000
            || u64::from(width) * u64::from(height) > 20_000_000
        {
            return Err(AppError::InvalidInput);
        }
        let p = self.page(page)?;
        unsafe {
            let lib = &self.engine.lib;
            let bitmap = call!(
                lib,
                "FPDFBitmap_Create",
                unsafe extern "C" fn(i32, i32, i32) -> Handle,
                width as i32,
                height as i32,
                0
            );
            if bitmap.is_null() {
                return Err(AppError::ParseFailed);
            }
            call!(
                lib,
                "FPDFBitmap_FillRect",
                unsafe extern "C" fn(Handle, i32, i32, i32, i32, u32) -> i32,
                bitmap,
                0,
                0,
                width as i32,
                height as i32,
                0xffffffff
            );
            // No FORM_*, action, attachment, JavaScript, XFA or annotation calls.
            call!(
                lib,
                "FPDF_RenderPageBitmap",
                unsafe extern "C" fn(Handle, Handle, i32, i32, i32, i32, i32, i32),
                bitmap,
                p.h,
                0,
                0,
                width as i32,
                height as i32,
                0,
                0
            );
            let stride = call!(
                lib,
                "FPDFBitmap_GetStride",
                unsafe extern "C" fn(Handle) -> i32,
                bitmap
            );
            let buffer = call!(
                lib,
                "FPDFBitmap_GetBuffer",
                unsafe extern "C" fn(Handle) -> *const u8,
                bitmap
            );
            let result = if buffer.is_null() || stride < (width * 4) as i32 {
                Err(AppError::ParseFailed)
            } else {
                let raw = std::slice::from_raw_parts(buffer, (stride as usize) * height as usize);
                let mut image = RgbImage::new(width, height);
                for (x, y, pixel) in image.enumerate_pixels_mut() {
                    let pos = y as usize * stride as usize + x as usize * 4;
                    *pixel = image::Rgb([raw[pos + 2], raw[pos + 1], raw[pos]]);
                }
                Ok(image)
            };
            call!(
                lib,
                "FPDFBitmap_Destroy",
                unsafe extern "C" fn(Handle),
                bitmap
            );
            result
        }
    }
}

impl shixu_native::attachments::pdf::PdfReader for Pdfium {
    type Document<'a> = Document<'a>;
    fn open<'a>(&'a self, bytes: &[u8]) -> AppResult<Document<'a>> {
        Pdfium::open(self, bytes.to_vec())
    }
}
