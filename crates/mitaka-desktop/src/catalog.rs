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
        Ok(Self {
            root: root.to_path_buf(),
            stars,
            by_hip,
            constellations,
            ephemeris,
        })
    }
}
