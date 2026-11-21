use mitaka_core::catalog::{self, Constellation, Star};
use mitaka_core::ephemeris::Cf96Ephemeris;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

pub struct SkyData {
    pub root: PathBuf,
    pub stars: Vec<Star>,
    pub by_hip: HashMap<i32, usize>,
    pub constellations: Vec<Constellation>,
    pub ephemeris: Cf96Ephemeris,
}
impl SkyData {
    pub fn validate_date(&self, jd: f64) -> io::Result<()> {
        if !(mitaka_core::lunar::LUNAR_MINIMUM_JD..=mitaka_core::lunar::LUNAR_MAXIMUM_JD)
            .contains(&jd)
        {
            return Err(io::Error::other("日時が月位置計算の有効範囲外です"));
        }
        for i in 0..9 {
            let [lo, hi] = self.ephemeris.time_range(i)?;
            if !(lo..=hi).contains(&jd) {
                return Err(io::Error::other("日時がCF96暦表の有効範囲外です"));
            }
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
        Ok(Self {
            root: root.to_path_buf(),
            stars,
            by_hip,
            constellations,
            ephemeris,
        })
    }
}
