use eframe::egui;
use image::{DynamicImage, ImageFormat, RgbImage};
use std::{
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

pub fn output_path(candidate: &Path, data: &Path) -> io::Result<PathBuf> {
    let parent = candidate
        .parent()
        .ok_or_else(|| io::Error::other("出力先を確認できません"))?
        .canonicalize()?;
    let target = parent.join(
        candidate
            .file_name()
            .ok_or_else(|| io::Error::other("ファイル名が必要です"))?,
    );
    let existing = match target.canonicalize() {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => target.clone(),
        Err(e) => return Err(e),
    };
    let data = data.canonicalize()?;
    for p in [&existing, &target] {
        if p.starts_with(&data)
            || p.ancestors().any(|a| {
                let ext = a.extension().and_then(|e| e.to_str()).unwrap_or("");
                let component = a.file_name().and_then(|v| v.to_str()).unwrap_or("");
                ext.eq_ignore_ascii_case("app")
                    || a.join("Contents/Info.plist").is_file()
                    || (a.join("data/hip.dat").is_file() && a.join("data/cf96.dat").is_file())
                    || (a.join("hip.dat").is_file() && a.join("cf96.dat").is_file())
                    || ((component.eq_ignore_ascii_case("official")
                        || component.eq_ignore_ascii_case("reference"))
                        && a.parent()
                            .and_then(Path::file_name)
                            .is_some_and(|s| s.eq_ignore_ascii_case("sources")))
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "原本またはアプリのディレクトリ内には保存できません",
            ));
        }
    }
    Ok(target)
}
pub fn save(
    image: &egui::ColorImage,
    path: &Path,
    root: &Path,
    crop: egui::Rect,
    scale: f32,
) -> io::Result<()> {
    let path = output_path(path, root)?;
    let format = ImageFormat::from_path(&path).map_err(io::Error::other)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp
    ) {
        return Err(io::Error::other("保存形式は PNG・JPEG・BMP です"));
    }
    let x = (crop.left() * scale)
        .round()
        .clamp(0., image.width() as f32) as usize;
    let y = (crop.top() * scale)
        .round()
        .clamp(0., image.height() as f32) as usize;
    let right = (crop.right() * scale)
        .round()
        .clamp(x as f32, image.width() as f32) as usize;
    let bottom = (crop.bottom() * scale)
        .round()
        .clamp(y as f32, image.height() as f32) as usize;
    if x == right || y == bottom {
        return Err(io::Error::other("画像サイズが不正です"));
    }
    let mut bytes = Vec::with_capacity((right - x) * (bottom - y) * 3);
    for row in y..bottom {
        for pixel in &image.pixels[row * image.width() + x..row * image.width() + right] {
            bytes.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
        }
    }
    let rgb = RgbImage::from_raw((right - x) as u32, (bottom - y) as u32, bytes)
        .ok_or_else(|| io::Error::other("RGB画像が不正です"))?;
    let mut temp = NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| io::Error::other("出力フォルダがありません"))?,
    )?;
    {
        let mut out = BufWriter::new(temp.as_file_mut());
        DynamicImage::ImageRgb8(rgb)
            .write_to(&mut out, format)
            .map_err(io::Error::other)?;
        out.flush()?;
    }
    temp.persist(path)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_excludes_original_and_preserves_prior_image_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let image = egui::ColorImage::new([4, 4], egui::Color32::RED);
        let crop = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(4., 4.));
        assert!(save(&image, &data.join("a.png"), &data, crop, 1.).is_err());
        for ext in ["png", "jpg", "bmp"] {
            let path = dir.path().join(format!("a.{ext}"));
            save(&image, &path, &data, crop, 1.).unwrap();
            assert_eq!(image::open(&path).unwrap().width(), 4);
            let old = std::fs::read(&path).unwrap();
            assert!(save(&image, &path, &data, egui::Rect::ZERO, 1.).is_err());
            assert_eq!(std::fs::read(path).unwrap(), old);
        }
    }
}
