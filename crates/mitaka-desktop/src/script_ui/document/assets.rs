//! 文書の隣にある宣言済みの CSS・画像だけを directory capability から取得する。
use super::{decode, invalid};
use cap_std::fs::{Dir, OpenOptions};
use eframe::egui;
use same_file::Handle;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

struct StyleFile {
    path: PathBuf,
    id: Handle,
    digest: [u8; 32],
}
pub(super) struct Files {
    dir: Option<Dir>,
    parent: PathBuf,
    css: Vec<StyleFile>,
    pub images: Vec<Picture>,
    index: HashMap<PathBuf, usize>,
    pixels: usize,
}
pub(super) struct Picture {
    image: Option<egui::ColorImage>,
    texture: Option<egui::TextureHandle>,
    pub size: egui::Vec2,
}
impl Picture {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui, requested: egui::Vec2) {
        if let Some(image) = self.image.take() {
            self.texture = Some(ui.ctx().load_texture(
                "HTML local image",
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
        if let Some(t) = &self.texture {
            let scale = (ui.available_width().max(16.) / requested.x).min(1.);
            ui.image((t.id(), requested * scale));
        }
    }
}
fn relative(text: &str, suffix: &[&str]) -> io::Result<PathBuf> {
    let p = Path::new(text);
    if text.is_empty()
        || text.len() > 1024
        || text.contains([':', '\\', '?', '#', '%', '\0'])
        || p.components()
            .any(|p| !matches!(p, Component::Normal(_) | Component::CurDir))
        || !p
            .extension()
            .and_then(|v| v.to_str())
            .is_some_and(|e| suffix.iter().any(|s| e.eq_ignore_ascii_case(s)))
    {
        return Err(invalid("文書と同じフォルダー内の相対ファイル名が必要です"));
    }
    Ok(p.to_path_buf())
}
impl Files {
    pub fn new(dir: &Path) -> Self {
        Self {
            parent: dir.to_path_buf(),
            dir: Dir::open_ambient_dir(dir, cap_std::ambient_authority()).ok(),
            css: vec![],
            images: vec![],
            index: HashMap::new(),
            pixels: 0,
        }
    }
    fn file(&self, path: &Path, max: u64) -> io::Result<std::fs::File> {
        let mut opts = OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_NONBLOCK);
        }
        let f = self
            .dir
            .as_ref()
            .ok_or_else(|| invalid("HTML のフォルダーを開けません"))?
            .open_with(path, &opts)?
            .into_std();
        let m = f.metadata()?;
        if !m.is_file() || m.len() > max {
            return Err(invalid("ローカル資源のサイズ上限を超えました"));
        }
        Ok(f)
    }
    fn style_bytes(&self, path: &Path) -> io::Result<(Handle, Vec<u8>)> {
        const MAX: u64 = 256 * 1024;
        let f = self.file(path, MAX)?;
        let handle = Handle::from_file(f.try_clone()?)?;
        let mut bytes = vec![];
        f.take(MAX + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX {
            return Err(invalid("CSS が大きすぎます"));
        }
        Ok((handle, bytes))
    }
    pub fn css(&mut self, href: &str) -> io::Result<String> {
        if self.css.len() >= 8 {
            return Err(invalid("CSS の枚数が上限を超えました"));
        }
        let path = relative(href, &["css"])?;
        let (id, bytes) = self.style_bytes(&path)?;
        let text = decode(&bytes)?.into_owned();
        self.css.push(StyleFile {
            path,
            id,
            digest: Sha256::digest(bytes).into(),
        });
        Ok(text)
    }
    pub fn current(&self) -> bool {
        if !self.css.is_empty() {
            let held = self
                .dir
                .as_ref()
                .and_then(|d| d.try_clone().ok())
                .and_then(|d| Handle::from_file(d.into_std_file()).ok());
            if held.is_none() || held != Handle::from_path(&self.parent).ok() {
                return false;
            }
        }
        self.css.iter().all(|s| {
            self.style_bytes(&s.path).is_ok_and(|(id, bytes)| {
                id == s.id && <[u8; 32]>::from(Sha256::digest(bytes)) == s.digest
            })
        })
    }
    pub fn image(&mut self, href: &str) -> Option<usize> {
        let path = relative(href, &["png", "jpg", "jpeg"]).ok()?;
        if let Some(i) = self.index.get(&path) {
            return Some(*i);
        }
        if self.images.len() >= 32 {
            return None;
        }
        const MAX: u64 = 4 * 1024 * 1024;
        let file = self.file(&path, MAX).ok()?;
        let mut bytes = vec![];
        file.take(MAX + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX {
            return None;
        }
        let image =
            crate::media_preview::image::document_image(&path, &bytes, MAX - self.pixels as u64)
                .ok()?;
        self.pixels += image.pixels.len();
        let i = self.images.len();
        let size = egui::vec2(image.size[0] as f32, image.size[1] as f32);
        self.images.push(Picture {
            image: Some(image),
            texture: None,
            size,
        });
        self.index.insert(path, i);
        Some(i)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    #[test]
    fn only_linked_local_files_can_supply_styles_and_file_replacements_revoke() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("other.css"), "a{display:none}").unwrap();
        symlink(outside.path(), d.path().join("escape")).unwrap();
        let mut files = Files::new(d.path());
        assert!(files.css("escape/other.css").is_err());
        assert!(files.css("../other.css").is_err());
        assert!(files.css("https://example.invalid/other.css").is_err());
        let p = d.path().join("style.css");
        fs::write(&p, "a{color:#000}").unwrap();
        files.css("style.css").unwrap();
        assert!(files.current());
        fs::rename(&p, d.path().join("old.css")).unwrap();
        fs::write(&p, "a{color:#000}").unwrap();
        assert!(!files.current());
    }
}
