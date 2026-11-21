use eframe::egui;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::{
    fs::OpenOptions,
    io::{self, Cursor},
    path::Path,
};

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn load_rgba(path: &Path, landscape: bool) -> io::Result<image::RgbaImage> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NONBLOCK);
    }
    let file = opts.open(path)?;
    decode(path, file, landscape)
}
fn decode(path: &Path, file: std::fs::File, landscape: bool) -> io::Result<image::RgbaImage> {
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_BYTES {
        return Err(invalid(
            "画像は 64 MiB 以下の通常ファイルを選択してください",
        ));
    }
    let mut bytes = vec![0; meta.len() as usize];
    let mut offset = 0;
    while offset < bytes.len() {
        #[cfg(unix)]
        let count =
            std::os::unix::fs::FileExt::read_at(&file, &mut bytes[offset..], offset as u64)?;
        #[cfg(windows)]
        let count =
            std::os::windows::fs::FileExt::seek_read(&file, &mut bytes[offset..], offset as u64)?;
        if count == 0 {
            return Err(invalid("画像の読込中にサイズが変わりました"));
        }
        offset += count;
    }
    if file.metadata()?.len() != meta.len() {
        return Err(invalid("画像の読込中にサイズが変わりました"));
    }
    let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
    match reader.format() {
        Some(ImageFormat::Png) if bytes.ends_with(b"\0\0\0\0IEND\xaeB\x60\x82") => {}
        Some(ImageFormat::Jpeg) if bytes.ends_with(&[0xff, 0xd9]) => {}
        Some(ImageFormat::Bmp)
            if landscape && bytes.get(28..30).is_some_and(|v| matches!(v, [24 | 32, 0])) => {}
        None if landscape
            && path
                .extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("tga"))
            && bytes.len() >= 18
            && matches!(bytes[2], 2 | 10)
            && matches!(bytes[16], 16 | 24 | 32) =>
        {
            reader.set_format(ImageFormat::Tga);
        }
        _ if landscape => return Err(invalid("完全なPNG/JPEG/BMP/TGA風景画像が必要です")),
        _ => return Err(invalid("完全な PNG または JPEG 画像を選択してください")),
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(io::Error::other)?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(invalid("画像の画素数が上限を超えました"));
    }
    let orientation = decoder.orientation().map_err(io::Error::other)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(io::Error::other)?;
    image.apply_orientation(orientation);
    if image.width() > 1024 || image.height() > 1024 {
        image = image.thumbnail(1024, 1024);
    }
    Ok(image.to_rgba8())
}

pub(crate) fn load_landscape(path: &Path) -> io::Result<image::RgbaImage> {
    load_rgba(path, true)
}

#[cfg(test)]
pub fn load(path: &Path) -> io::Result<egui::ColorImage> {
    load_selected(path, std::fs::File::open(path)?)
}
pub fn load_selected(path: &Path, file: std::fs::File) -> io::Result<egui::ColorImage> {
    let image = decode(path, file, false)?;
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        &image,
    ))
}

pub struct Preview {
    texture: egui::TextureHandle,
    zoom: f32,
    angle: f32,
    opacity: f32,
    pan: egui::Vec2,
}
impl Preview {
    pub fn new(ctx: &egui::Context, image: egui::ColorImage) -> Self {
        Self {
            texture: ctx.load_texture("media image", image, egui::TextureOptions::LINEAR),
            zoom: 1.,
            angle: 0.,
            opacity: 1.,
            pan: egui::Vec2::ZERO,
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let size = egui::vec2(ui.available_width().max(160.), 300.);
        let (area, response) = ui.allocate_exact_size(size, egui::Sense::drag());
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        let mut canvas = ui.new_child(egui::UiBuilder::new().max_rect(area));
        canvas.set_clip_rect(area.intersect(ui.clip_rect()));
        canvas
            .painter()
            .rect_filled(area, 0., egui::Color32::from_gray(15));
        let original = self.texture.size_vec2();
        let scale = (area.width() / original.x).min(area.height() / original.y) * self.zoom;
        let rect = egui::Rect::from_center_size(area.center() + self.pan, original * scale);
        egui::Image::new(&self.texture)
            .rotate(-self.angle.to_radians(), egui::vec2(0.5, 0.5))
            .tint(egui::Color32::from_white_alpha(
                (self.opacity * 255.).round() as u8,
            ))
            .paint_at(&canvas, rect);
        ui.add(
            egui::Slider::new(&mut self.zoom, 0.05..=20.)
                .logarithmic(true)
                .text("倍率"),
        );
        ui.add(egui::Slider::new(&mut self.angle, -180. ..=180.).text("回転 °"));
        ui.add(egui::Slider::new(&mut self.opacity, 0. ..=1.).text("不透明度"));
        if ui.button("画像の位置・表示をリセット").clicked() {
            self.pan = egui::Vec2::ZERO;
            self.zoom = 1.;
            self.angle = 0.;
            self.opacity = 1.;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_bounded_pixels_without_writing_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("日本語.png");
        image::RgbaImage::from_pixel(1300, 650, image::Rgba([12, 34, 56, 128]))
            .save(&path)
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let img = load(&path).unwrap();
        assert_eq!(img.size, [1024, 512]);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(load(&path).is_err());
    }
}
