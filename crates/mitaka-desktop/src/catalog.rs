use mitaka_core::catalog::{self, Constellation, Star};
use mitaka_core::ephemeris::Cf96Ephemeris;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

pub struct SkyData {
    pub layers: crate::layers::catalog::LayersCatalog,
    pub root: PathBuf,
    pub date_range: [f64; 2],
    pub stars: Vec<Star>,
    pub by_hip: HashMap<i32, usize>,
    pub constellations: Vec<Constellation>,
    pub ephemeris: Cf96Ephemeris,
}
impl SkyData {
    pub fn validate_date(&self, jd: f64) -> io::Result<()> {
        if !(self.date_range[0]..=self.date_range[1]).contains(&jd) {
            return Err(io::Error::other(
                "日時が月位置計算またはCF96暦表の有効範囲外です",
            ));
        }
        Ok(())
    }
    pub fn open(root: &Path) -> io::Result<Self> {
        let stars = catalog::load_hipparcos(&root.join("hip.dat"))?;
        if stars.is_empty() || stars.iter().any(|s| !(-90. ..=90.).contains(&s.dec_deg)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "恒星カタログの赤緯または件数が不正です",
            ));
        }
        let mut by_hip = HashMap::with_capacity(stars.len());
        for (index, star) in stars.iter().enumerate() {
            by_hip.entry(star.hip).or_insert(index);
        }
        let constellation_file = root.join("constellation_HIP.dat");
        let constellations = match catalog::load_constellations(&constellation_file) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            result => result?,
        };
        let ephemeris = Cf96Ephemeris::load(&root.join("cf96.dat"))?;
        if ephemeris.planet_count() != 9 {
            return Err(io::Error::other("CF96は9天体を必要とします"));
        }
        let mut date_range = [
            mitaka_core::lunar::LUNAR_MINIMUM_JD,
            mitaka_core::lunar::LUNAR_MAXIMUM_JD,
        ];
        for i in 0..ephemeris.planet_count() {
            let [lo, hi] = ephemeris.time_range(i)?;
            date_range = [lo.max(date_range[0]), hi.min(date_range[1])];
        }
        if date_range[0] >= date_range[1] {
            return Err(io::Error::other("暦表と月位置計算の有効範囲が重なりません"));
        }
        Ok(Self {
            layers: crate::layers::catalog::LayersCatalog::load(root, &constellations),
            date_range,
            root: root.to_path_buf(),
            stars,
            by_hip,
            constellations,
            ephemeris,
        })
    }
}
