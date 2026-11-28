//! Mitaka_plus f9fe624 catalog_browser_model.mm と同じ種類順・500行単位で検索する。
use super::{
    catalog::{Catalog, SearchRows},
    display::Catalogs,
    Kind,
};
use eframe::egui;
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

const PAGE_SIZE: usize = 500;
type Reference = (Kind, usize);
#[derive(Default)]
struct Page {
    rows: Vec<Reference>,
    matches: usize,
    total: usize,
}

fn page(groups: &[SearchRows], query: &str, offset: usize, cancelled: &AtomicBool) -> Option<Page> {
    let words = SearchRows::words(query);
    let mut page = Page::default();
    for group in groups {
        page.total += group.total;
        let Some(words) = group.remaining_words(&words) else {
            continue;
        };
        if words.is_empty() {
            let first = offset.saturating_sub(page.matches).min(group.total);
            let end = (first + PAGE_SIZE - page.rows.len()).min(group.total);
            page.rows.extend((first..end).map(|i| (group.kind, i)));
            page.matches += group.total;
            continue;
        }
        for i in 0..group.total {
            if i % 256 == 0 && cancelled.load(Ordering::Relaxed) {
                return None;
            }
            if group.matches(i, &words) {
                if page.matches >= offset && page.rows.len() < PAGE_SIZE {
                    page.rows.push((group.kind, i));
                }
                page.matches += 1;
            }
        }
    }
    (!cancelled.load(Ordering::Relaxed)).then_some(page)
}

struct Job {
    rx: mpsc::Receiver<Page>,
    stop: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub(super) struct Search {
    offset: usize,
    page: Option<Page>,
    job: Option<Job>,
}
impl Search {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    fn move_page(&mut self, offset: usize) {
        self.offset = offset;
        self.page = None;
        self.job = None;
    }
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        query: &mut String,
        scope: Option<Kind>,
        sky: &crate::catalog::SkyData,
        cache: &mut Catalogs,
        selected: Option<Reference>,
    ) -> Option<(Kind, usize, Rc<Catalog>)> {
        if ui
            .add(egui::TextEdit::singleline(query).hint_text("名称・HIP番号・カタログ番号"))
            .changed()
        {
            self.reset();
        }
        let mut loaded = Vec::new();
        let mut waiting = false;
        for kind in Kind::GENERAL
            .into_iter()
            .filter(|k| scope.is_none_or(|s| s == *k))
        {
            match cache.get(kind, sky, ui.ctx()) {
                Ok(Some(c)) => loaded.push((kind, c)),
                Ok(None) => waiting = true,
                Err(e) => {
                    ui.collapsing(format!("{}の読込情報", kind.label()), |ui| ui.label(e));
                }
            }
        }
        if waiting {
            ui.spinner();
            return None;
        }
        if self.page.is_none() && self.job.is_none() {
            let groups: Vec<_> = loaded.iter().map(|(k, c)| c.search_rows(*k)).collect();
            let query = query.clone();
            let offset = self.offset;
            let ctx = ui.ctx().clone();
            let (tx, rx) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            std::thread::spawn(move || {
                if let Some(page) = page(&groups, &query, offset, &flag) {
                    let _ = tx.send(page);
                    ctx.request_repaint();
                }
            });
            self.job = Some(Job { rx, stop });
        }
        if let Some(job) = &self.job {
            match job.rx.try_recv() {
                Ok(result) => {
                    self.page = Some(result);
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.spinner();
                    return None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    ui.label("検索スレッドが終了しました");
                    if ui.button("再試行").clicked() {
                        self.job = None;
                    }
                    return None;
                }
            }
        }
        let page = self.page.as_ref()?;
        let mut next = self.offset;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.offset > 0, egui::Button::new("前へ"))
                .clicked()
            {
                next = self.offset.saturating_sub(PAGE_SIZE);
            }
            ui.label(format!(
                "{}–{} / {} 件（全 {} 件）",
                self.offset + usize::from(page.matches > 0),
                self.offset + page.rows.len(),
                page.matches,
                page.total
            ));
            if ui
                .add_enabled(
                    self.offset + page.rows.len() < page.matches,
                    egui::Button::new("次へ"),
                )
                .clicked()
            {
                next = self.offset + PAGE_SIZE;
            }
        });
        if next != self.offset {
            self.move_page(next);
            return None;
        }
        let mut result = None;
        let height = ui.text_style_height(&egui::TextStyle::Body) + 4.;
        egui::ScrollArea::vertical()
            .id_salt(("catalog-page", self.offset))
            .max_height(210.)
            .show_rows(ui, height, page.rows.len(), |ui, range| {
                for &(kind, i) in &page.rows[range] {
                    if let Some((_, c)) = loaded.iter().find(|(k, _)| *k == kind) {
                        if ui
                            .selectable_label(selected == Some((kind, i)), c.title(i))
                            .clicked()
                        {
                            result = Some((kind, i, c.clone()));
                        }
                    }
                }
            });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_search_and_saved_membership_use_the_same_gui_table() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let mut hip = [0; mitaka_core::catalog::HIP_RECORD_BYTES];
        hip[..4].copy_from_slice(&32349i32.to_le_bytes());
        std::fs::write(root.join("hip.dat"), hip).unwrap();
        let files = ["star_names_HIP.dat", "star_names.dat", "star_names_jp.dat"];
        for (i, file) in files.iter().enumerate() {
            // 下位の表と異なる別名で、共存時に同じ表が選ばれることを確認する。
            let name = ["Legacy", "Sirius", "SiriusJp"][i];
            std::fs::write(root.join(file), format!(" 32349*{name}")).unwrap();
            let displayed = crate::layers::catalog::LayersCatalog::load(root, &[]);
            assert_eq!(displayed.names.get(&32349).unwrap(), name);
            assert!(displayed.known_star_names.contains(&32349));
            let cat = Catalog::load(Kind::Stars, root).unwrap();
            assert_eq!(cat.search(name), Some(vec![0]));
            assert!(cat.info(0, crate::J2000).contains(file));
        }
        std::fs::write(root.join(files[2]), b"invalid star name").unwrap();
        assert!(crate::layers::catalog::gui_star_names(root).is_err());
        for file in files {
            std::fs::remove_file(root.join(file)).unwrap();
        }
        let displayed = crate::layers::catalog::LayersCatalog::load(root, &[]);
        assert!(displayed.known_star_names.is_empty());
        let cat = Catalog::load(Kind::Stars, root).unwrap();
        assert_eq!(cat.search("Sirius"), Some(vec![]));
        assert_eq!(cat.search("32349"), Some(vec![0]));
    }

    #[test]
    #[ignore = "旧版の日本語・英語固有名表を含む実カタログが必要"]
    fn restores_native_sirius_selection_with_either_gui_table() {
        let original = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let mut sky = crate::SkyData::open(&original).unwrap();
        let mut state = crate::session::Session::default();
        state.0["selected_star_name_hips"] = serde_json::json!([32349]);
        let bytes = serde_json::to_vec(&state.0).unwrap();
        let state = crate::session::Session::decode(&bytes).unwrap();
        for name in ["star_names.dat", "star_names_jp.dat"] {
            let temp = tempfile::tempdir().unwrap();
            sky.root = temp.path().to_owned();
            std::fs::copy(original.join(name), sky.root.join(name)).unwrap();
            sky.layers =
                crate::layers::catalog::LayersCatalog::load(&sky.root, &sky.constellations);
            assert_eq!(sky.layers.known_star_names.len(), 174);
            assert!(crate::session_ui::DisplayLayers::prepare(&state, &sky).is_ok());
            let c = Catalog::load_with_stars(Kind::Stars, &sky.root, Some(&sky.stars)).unwrap();
            assert_eq!(c.search("Sirius").unwrap().len(), 1);
        }
    }

    #[test]
    fn pages_all_stars_and_planets_and_joins_optional_names_by_hip() {
        let root = tempfile::tempdir().unwrap();
        let mut hip = vec![];
        for id in 1..=505i32 {
            let mut record = [0; mitaka_core::catalog::HIP_RECORD_BYTES];
            record[..4].copy_from_slice(&id.to_le_bytes());
            hip.extend(record);
        }
        let root = root.path();
        std::fs::write(root.join("hip.dat"), hip).unwrap();
        assert_eq!(Catalog::load(Kind::Stars, root).unwrap().len(), 505);
        std::fs::write(root.join("star_names_HIP.dat"), b"     2*Vega  Vega\n").unwrap();
        let planets = Catalog::load(Kind::Planets, root).unwrap();
        let stars = Catalog::load(Kind::Stars, root).unwrap();
        let groups = [
            planets.search_rows(Kind::Planets),
            stars.search_rows(Kind::Stars),
        ];
        let stop = AtomicBool::new(false);
        assert_eq!(page(&groups, "hip.dat", 0, &stop).unwrap().matches, 505);
        assert_eq!(
            page(&groups, "star_names_HIP.dat", 0, &stop).unwrap().rows,
            vec![(Kind::Stars, 1)]
        );
        // f9fe624 は「太陽」を種類名「太陽系」の部分一致としても扱う。
        let solar_system: Vec<_> = (0..11).map(|i| (Kind::Planets, i)).collect();
        for query in ["太陽", "太陽 planet", "太陽系", "planet"] {
            assert_eq!(
                page(&groups, query, 0, &stop).unwrap().rows,
                solar_system,
                "{query}"
            );
        }
        for (query, reference) in [
            ("Sun", (Kind::Planets, 0)),
            ("Ｓｕｎ", (Kind::Planets, 0)),
            ("月", (Kind::Planets, 10)),
            ("moon", (Kind::Planets, 10)),
            ("火星", (Kind::Planets, 4)),
            ("ＭＡＲＳ", (Kind::Planets, 4)),
            ("HIP505", (Kind::Stars, 504)),
            ("HIP 2 vega", (Kind::Stars, 1)),
        ] {
            let hits = page(&groups, query, 0, &stop).unwrap();
            assert_eq!(hits.rows, [reference], "{query}");
        }
        let first = page(&groups, "", 0, &stop).unwrap();
        let second = page(&groups, "", 500, &stop).unwrap();
        assert_eq!(
            (first.total, first.matches, first.rows.len()),
            (516, 516, 500)
        );
        assert_eq!(first.rows.last(), Some(&(Kind::Stars, 488)));
        assert_eq!(second.rows.first(), Some(&(Kind::Stars, 489)));
        assert_eq!(second.rows.len(), 16);
        stop.store(true, Ordering::Relaxed);
        assert!(page(&groups, "HIP", 0, &stop).is_none());
    }

    #[test]
    fn searches_ngc_and_ic_by_source_without_matching_the_record_ordinal() {
        let root = tempfile::tempdir().unwrap();
        let mut ngc_record = [0; 21];
        ngc_record[..2].copy_from_slice(&76i16.to_le_bytes());
        ngc_record[2] = 1;
        let mut ic_record = ngc_record;
        ic_record[2] = 0;
        std::fs::write(
            root.path().join("ngc.dat"),
            [ngc_record, ic_record].concat(),
        )
        .unwrap();
        let catalog = Catalog::load(Kind::Ngc, root.path()).unwrap();
        let groups = [catalog.search_rows(Kind::Ngc)];
        let stop = AtomicBool::new(false);
        for (query, expected) in [("NGC.DAT", 2), ("NGC", 1), ("ngc.dat 1", 0)] {
            assert_eq!(
                page(&groups, query, 0, &stop).unwrap().matches,
                expected,
                "{query}"
            );
        }
    }

    #[test]
    fn editing_a_query_discards_an_already_queued_page() {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut search = Search {
            offset: PAGE_SIZE,
            page: None,
            job: Some(Job {
                rx,
                stop: stop.clone(),
            }),
        };
        tx.send(Page::default()).unwrap();
        search.reset();
        assert!(stop.load(Ordering::Relaxed) && search.page.is_none() && search.job.is_none());
        assert_eq!(search.offset, 0);
    }

    #[test]
    #[ignore = "MITAKA_DATA_ROOT に実データを指定して実行"]
    fn supplied_unnamed_star_maps_to_its_observed_position() {
        let root = std::path::PathBuf::from(std::env::var_os("MITAKA_DATA_ROOT").unwrap());
        let sky = crate::catalog::SkyData::open(&root).unwrap();
        let star = sky
            .stars
            .iter()
            .find(|s| !sky.layers.known_star_names.contains(&s.hip))
            .unwrap();
        let c = Catalog::load_with_stars(Kind::Stars, &root, Some(&sky.stars)).unwrap();
        assert_eq!((c.len(), sky.layers.known_star_names.len()), (117956, 174));
        let row = star.source_row - 1;
        let hits = page(
            &[c.search_rows(Kind::Stars)],
            &format!("HIP {}", star.hip),
            0,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(hits.rows.contains(&(Kind::Stars, row)));
        let super::super::Position::Sky(coords, Some(hip)) =
            c.locate(row, &sky, crate::J2000).unwrap()
        else {
            panic!("unnamed HIP should be locatable");
        };
        assert_eq!(hip, star.hip);
        assert_eq!(coords, [f64::from(star.ra_deg), f64::from(star.dec_deg)]);
    }
}
