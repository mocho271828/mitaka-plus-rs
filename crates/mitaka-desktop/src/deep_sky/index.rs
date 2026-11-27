//! sky_catalog_index.cpp の5度区画。赤経0度をまたぐ視野も全記録を集計する。
use crate::projection::{Camera, View};
use mitaka_core::extended_catalog::NgcRecord;

pub(super) trait Coordinates {
    fn coords(&self) -> [f32; 2];
}
impl Coordinates for NgcRecord {
    fn coords(&self) -> [f32; 2] {
        [self.ra_deg, self.dec_deg]
    }
}
impl Coordinates for [f32; 2] {
    fn coords(&self) -> [f32; 2] {
        *self
    }
}
pub(super) struct Index<T> {
    bins: Vec<Vec<T>>,
}
impl<T: Coordinates> Index<T> {
    pub(super) fn new(rows: impl IntoIterator<Item = T>) -> Self {
        let mut bins: Vec<Vec<T>> = (0..72 * 36).map(|_| Vec::new()).collect();
        for row in rows {
            let [ra, dec] = row.coords().map(f64::from);
            let x = (ra / 5.) as usize;
            let y = (((dec + 90.) / 5.) as usize).min(35);
            bins[y * 72 + x].push(row);
        }
        Self { bins }
    }
    pub(super) fn visible(&self, camera: Camera, mut project: impl FnMut(&T, [f64; 2])) {
        for (i, rows) in self.bins.iter().enumerate() {
            let dec = (i / 72) as f64 * 5. - 87.5;
            let ra = (i % 72) as f64 * 5. + 2.5;
            let offset = (ra - camera.longitude + 180.).rem_euclid(360.) - 180.;
            if (dec - camera.latitude).abs() > 90. / camera.zoom + 2.5
                || offset.abs() > 180. / camera.zoom + 2.5
            {
                continue;
            }
            for row in rows {
                let position = camera.position(View::Sky, row.coords().map(f64::from));
                if position.iter().all(|v| (0. ..=1.).contains(v)) {
                    project(row, position);
                }
            }
        }
    }
}
pub(super) fn density(
    index: &Index<[f32; 2]>,
    camera: Camera,
    [width, height]: [usize; 2],
) -> Vec<u32> {
    let mut counts = vec![0; width * height];
    index.visible(camera, |_, [x, y]| {
        let col = ((x * width as f64) as usize).min(width - 1);
        let row = ((y * height as f64) as usize).min(height - 1);
        counts[row * width + col] += 1;
    });
    counts
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edges_and_wrapping_match_an_exhaustive_projection() {
        let mut rows = vec![[0., 90.], [180., -90.], [0., 0.], [359.99, 0.], [0., 0.]];
        for ra in 0..360 {
            for dec in (-90..=90).step_by(5) {
                rows.push([ra as f32, dec as f32]);
            }
        }
        let index = Index::new(rows.iter().copied());
        for (center, lat, zoom) in [(180., 0., 1.), (0., 0., 4.), (9., 80., 20.)] {
            let cam = Camera {
                longitude: center,
                latitude: lat,
                zoom,
                ..Default::default()
            };
            let counts = density(&index, cam, [48, 24]);
            let mut expected = vec![0; counts.len()];
            for p in &rows {
                let [x, y] = cam.position(View::Sky, p.map(f64::from));
                if (0. ..=1.).contains(&x) && (0. ..=1.).contains(&y) {
                    expected[((y * 24.) as usize).min(23) * 48 + ((x * 48.) as usize).min(47)] += 1;
                }
            }
            assert_eq!(counts, expected);
        }
    }
}
