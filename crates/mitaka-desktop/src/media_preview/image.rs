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
    decode_bytes(path, &bytes, landscape, 32768, MAX_PIXELS)
}
fn decode_bytes(
    path: &Path,
    bytes: &[u8],
    landscape: bool,
    max_edge: u32,
    max_pixels: u64,
) -> io::Result<image::RgbaImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
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
    limits.max_image_width = Some(max_edge);
    limits.max_image_height = Some(max_edge);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(io::Error::other)?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > max_pixels {
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
    Ok(color_image(decode(path, file, false)?))
}
pub(crate) fn document_image(
    path: &Path,
    bytes: &[u8],
    max_pixels: u64,
) -> io::Result<egui::ColorImage> {
    Ok(color_image(decode_bytes(
        path, bytes, false, 2048, max_pixels,
    )?))
}
fn color_image(image: image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        &image,
    )
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
        let controls_height = 4. * (ui.spacing().interact_size.y + ui.spacing().item_spacing.y);
        let size = egui::vec2(
            ui.available_width(),
            (ui.available_height() - controls_height).max(32.),
        );
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
    #[test]
    fn native_image_window_tracks_transforms_close_lua_and_data_reset() {
        use super::super::{Entry, Grant, Media};
        use egui::{Color32, Rect, ViewportEvent, ViewportId};
        use serde_json::json;
        use std::{cell::Cell, rc::Rc};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview.png");
        image::RgbaImage::from_pixel(8, 4, image::Rgba([12, 34, 56, 255]))
            .save(&path)
            .unwrap();
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        let mut media = Media::default();
        media
            .registrations
            .entries
            .push(Entry::direct(Grant::open(&path).unwrap()));
        let script = |m: &mut Media, command, value| {
            m.script(
                &json!({"family":"Images","command":command,"key":"visible","value":value}),
                &ctx,
            )
            .unwrap()["value"]
                .clone()
        };
        script(&mut media, "set", json!(true));
        let mut preview = Preview::new(&ctx, load(&path).unwrap());
        let texture_id = preview.texture.id();
        preview.zoom = 0.5;
        preview.opacity = 0.5;
        media.loading = None;
        media.image = Some(preview);

        let close = Rc::new(Cell::new(false));
        let draws = Rc::new(Cell::new(0));
        let bounds = Rc::new(Cell::new(Rect::NOTHING));
        let id = ViewportId::from_hash_of("mitaka-image-preview");
        egui::Context::set_immediate_viewport_renderer({
            let (close, draws, bounds) = (close.clone(), draws.clone(), bounds.clone());
            move |ctx, mut viewport| {
                assert_eq!(viewport.ids.this, id);
                assert_eq!(viewport.ids.parent, ViewportId::ROOT);
                draws.set(draws.get() + 1);
                let mut input = egui::RawInput {
                    viewport_id: id,
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 540.),
                    )),
                    ..Default::default()
                };
                let info = input.viewports.entry(id).or_default();
                info.parent = Some(ViewportId::ROOT);
                if close.get() {
                    info.events.push(ViewportEvent::Close);
                }
                let output = ctx.run(input, |child| (viewport.viewport_ui_cb)(child));
                for primitive in ctx.tessellate(output.shapes, output.pixels_per_point) {
                    if let egui::epaint::Primitive::Mesh(m) = primitive.primitive {
                        if m.texture_id == texture_id {
                            assert!(m
                                .vertices
                                .iter()
                                .all(|v| v.color == Color32::from_white_alpha(128)));
                            bounds.set(m.calc_bounds());
                        }
                    }
                }
            }
        });
        let frame = |m: &mut Media| {
            let _ = ctx.run(Default::default(), |ctx| m.frame(ctx));
        };
        frame(&mut media);
        assert_eq!(script(&mut media, "get", json!(null)), true);
        let first = bounds.get();
        assert!(first.width() > first.height() && first.width() > 32.);
        let preview = media.image.as_mut().unwrap();
        preview.zoom *= 0.5;
        preview.angle = 90.;
        preview.pan = egui::vec2(12., 8.);
        frame(&mut media);
        let second = bounds.get();
        assert!((second.width() - first.height() / 2.).abs() < 0.1);
        assert!((second.height() - first.width() / 2.).abs() < 0.1);
        assert!(
            second
                .center()
                .distance(first.center() + egui::vec2(12., 8.))
                < 0.1
        );

        close.set(true);
        frame(&mut media);
        assert_eq!(script(&mut media, "get", json!(null)), false);
        assert!(media.image.is_none());
        let count = draws.get();
        frame(&mut media);
        assert_eq!(draws.get(), count);

        close.set(false);
        script(&mut media, "set", json!(true));
        frame(&mut media);
        assert!(draws.get() > count);
        script(&mut media, "set", json!(false));
        assert_eq!(script(&mut media, "get", json!(null)), false);
        script(&mut media, "set", json!(true));
        media.reset(dir.path());
        let count = draws.get();
        frame(&mut media);
        assert_eq!(draws.get(), count);
        assert_eq!(script(&mut media, "get", json!(null)), false);
    }
}
