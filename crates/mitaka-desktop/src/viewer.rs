//! 共通の描画・入力設定。判定値は原版 ScriptWorker.mm / GridRenderer.mm と同じ。
use eframe::egui::{
    self,
    epaint::{tessellator::Tessellator, FontImage, Mesh, Shape},
    Color32, Pos2,
};
use std::{io, path::PathBuf};

#[derive(Default, Clone, Copy, PartialEq)]
pub enum Picking {
    None,
    Hover,
    #[default]
    Click,
}
impl Picking {
    pub fn from_index(value: u64) -> Option<Self> {
        [Self::None, Self::Hover, Self::Click]
            .get(value as usize)
            .copied()
    }
    pub fn pointer(self, response: &egui::Response) -> Option<Pos2> {
        match self {
            Self::None => None,
            Self::Hover if !response.dragged() => response.hover_pos(),
            Self::Click if response.clicked() => response.interact_pointer_pos(),
            _ => None,
        }
    }
    pub fn raw_pointer(self, ui: &egui::Ui, rect: egui::Rect) -> Option<Pos2> {
        if !ui.rect_contains_pointer(rect) {
            return None;
        }
        ui.input(|i| match self {
            Self::None => None,
            Self::Hover if !i.pointer.any_down() => i.pointer.hover_pos(),
            Self::Click if i.pointer.primary_clicked() => i.pointer.interact_pos(),
            _ => None,
        })
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("天体の選択");
            for (value, name) in [
                (Self::None, "無効"),
                (Self::Hover, "ホバー"),
                (Self::Click, "クリック"),
            ] {
                ui.selectable_value(self, value, name);
            }
        });
    }
}

#[derive(Default)]
pub struct Quality {
    font: Option<(FontImage, egui::TextureHandle)>,
}
impl Quality {
    pub fn apply(
        &mut self,
        ctx: &egui::Context,
        layer: egui::LayerId,
        options: &crate::layers::LayerOptions,
    ) {
        let points = options.antialias && options.point_smoothing;
        let lines = options.antialias && options.line_smoothing;
        let text = options.antialias && options.text_smoothing;
        if points && lines && text {
            return;
        }
        let atlas = ctx.fonts(|f| f.texture_atlas());
        let atlas = atlas.lock();
        let mut config = ctx.tessellation_options(|t| *t);
        config.feathering = false;
        config.prerasterized_discs = false;
        let mut renderer = Tessellator::new(ctx.pixels_per_point(), config, atlas.size(), vec![]);
        let glyphs = if text {
            None
        } else {
            let source = atlas.image();
            if !self.font.as_ref().is_some_and(|(old, _)| old == source) {
                let pixels = source
                    .pixels
                    .iter()
                    .map(|a| {
                        if *a >= 0.5 {
                            Color32::WHITE
                        } else {
                            Color32::TRANSPARENT
                        }
                    })
                    .collect();
                let data = egui::ColorImage {
                    size: source.size,
                    pixels,
                };
                let handle =
                    ctx.load_texture("mitaka-mono-font", data, egui::TextureOptions::NEAREST);
                self.font = Some((source.clone(), handle));
            }
            self.font.as_ref().map(|(_, handle)| handle.id())
        };
        drop(atlas);
        ctx.graphics_mut(|g| {
            let Some(list) = g.get_mut(layer) else { return };
            for i in 0..list.next_idx().0 {
                list.mutate_shape(egui::layers::ShapeIdx(i), |entry| {
                    let disable = match entry.shape {
                        Shape::Circle(_) | Shape::Ellipse(_) => !points,
                        Shape::Path(_)
                        | Shape::LineSegment { .. }
                        | Shape::QuadraticBezier(_)
                        | Shape::CubicBezier(_) => !lines,
                        Shape::Text(_) => !text,
                        _ => false,
                    };
                    if !disable {
                        return;
                    }
                    let is_text = matches!(entry.shape, Shape::Text(_));
                    let mut mesh = Mesh::default();
                    renderer.tessellate_shape(
                        std::mem::replace(&mut entry.shape, Shape::Noop),
                        &mut mesh,
                    );
                    if is_text {
                        if let Some(id) = glyphs {
                            mesh.texture_id = id;
                        }
                    }
                    entry.shape = Shape::mesh(mesh);
                });
            }
        });
    }
}

fn layout_file() -> io::Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or_else(|| io::Error::other("設定ディレクトリを特定できません"))?
        .join("mitaka-plus-rs/main-window.json"))
}
pub fn window_position(ctx: &egui::Context, save: bool) -> io::Result<()> {
    let path = layout_file()?;
    if save {
        let origin = ctx
            .input(|i| i.viewport().outer_rect.map(|r| r.min))
            .ok_or_else(|| io::Error::other("この表示環境ではウィンドウ位置を取得できません"))?;
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        let mut temp = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer(temp.as_file_mut(), &[origin.x, origin.y])?;
        temp.persist(path)?;
    } else {
        let bytes = std::fs::read(path)?;
        let [x, y]: [f32; 2] = serde_json::from_slice(&bytes)?;
        if !(x.is_finite() && y.is_finite()) {
            return Err(io::Error::other("ウィンドウ位置が不正です"));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
    }
    Ok(())
}
