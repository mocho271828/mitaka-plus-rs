//! Mitaka_plus f9fe624 galaxy_spatial_scene.cpp / GalaxyRenderer.mm.
use mitaka_core::galaxy_radiative::RadiativeModel;
use mitaka_core::{
    extended_catalog::{load_sdss, SdssKind},
    galaxy::{self, OurGalaxyModel},
};
use std::sync::Arc;
use std::{io, path::Path};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    MilkyWay,
    Clusters,
    Nearby,
    Sdss,
    Quasars,
}
impl Kind {
    pub const ALL: [Self; 5] = [
        Self::MilkyWay,
        Self::Clusters,
        Self::Nearby,
        Self::Sdss,
        Self::Quasars,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::MilkyWay => "天の川銀河",
            Self::Clusters => "球状星団",
            Self::Nearby => "近傍銀河",
            Self::Sdss => "SDSS 銀河",
            Self::Quasars => "SDSS クエーサー",
        }
    }
    pub fn file(self) -> &'static str {
        match self {
            Self::MilkyWay => "ourgalaxy_model.dat",
            Self::Clusters => "globular_cluster.dat",
            Self::Nearby => "NBG.dat",
            Self::Sdss => "SDSS_GAL.dat",
            Self::Quasars => "SDSS_QSO.dat",
        }
    }
}
pub(super) struct Object {
    pub position: [f64; 3],
    pub radius_pc: f64,
    pub row: usize,
    pub group: u8,
    pub name: Option<String>,
}
impl Object {
    pub fn label(&self, kind: Kind) -> String {
        self.name.clone().unwrap_or_else(|| {
            if kind == Kind::MilkyWay {
                format!("点群 {} · 原行 {}", self.group + 1, self.row)
            } else {
                format!("{} · 原行 {}", kind.name(), self.row)
            }
        })
    }
}
pub(super) struct Objects {
    pub radiation: Option<io::Result<Arc<RadiativeModel>>>,
    pub rows: Vec<Object>,
    pub source_rows: usize,
    pub radius: f64,
}
pub(super) fn objects(kind: Kind, root: &Path) -> io::Result<Objects> {
    let path = root.join(kind.file());
    let mut rows = Vec::new();
    let mut source_rows = 0;
    let mut radiation = None;
    let mut add = |row: usize,
                   ra: f32,
                   dec: f32,
                   distance: f32,
                   radius_pc: f64,
                   group: u8,
                   name: Option<String>| {
        source_rows += 1;
        if let Some(p) = galaxy::evaluate_position_pc(ra, dec, distance).position {
            rows.push(Object {
                position: p.ecliptic,
                radius_pc,
                row,
                group,
                name,
            });
        }
    };
    match kind {
        Kind::Clusters => {
            for (i, g) in galaxy::load_globular_clusters(&path)?
                .records
                .into_iter()
                .enumerate()
            {
                let name = String::from_utf8_lossy(&g.name_bytes)
                    .trim_matches(['\0', ' '])
                    .to_owned();
                add(
                    i + 1,
                    g.ra_deg,
                    g.dec_deg,
                    g.distance_pc,
                    f64::from(g.half_light_radius_pc).max(0.),
                    0,
                    Some(name),
                );
            }
        }
        Kind::Nearby => {
            for (i, g) in galaxy::load_nearby_galaxies(&path)?
                .records
                .into_iter()
                .enumerate()
            {
                let code = if g.catalog_code.is_ascii_graphic() {
                    char::from(g.catalog_code).to_string()
                } else {
                    g.catalog_code.to_string()
                };
                add(
                    i + 1,
                    g.ra_deg,
                    g.dec_deg,
                    g.distance_pc,
                    nearby_radius(g.distance_pc, g.diameter_arcmin),
                    0,
                    Some(format!("{code}{} · 原行 {}", g.catalog_number, i + 1)),
                );
            }
        }
        Kind::Sdss | Kind::Quasars => {
            let family = if kind == Kind::Sdss {
                SdssKind::Galaxy
            } else {
                SdssKind::Quasar
            };
            for (i, g) in load_sdss(&path, family)?.into_iter().enumerate() {
                add(
                    i + 1,
                    g.ra_deg,
                    g.dec_deg,
                    g.distance_pc,
                    0.,
                    if kind == Kind::Sdss {
                        sdss_class(g.classification_value)
                    } else {
                        0
                    },
                    None,
                );
            }
        }
        Kind::MilkyWay => {
            let model = OurGalaxyModel::load(&path)?;
            radiation = Some(RadiativeModel::from_model(&model).map(Arc::new));
            for (group, cloud) in model.clouds.into_iter().enumerate() {
                for (i, position) in cloud.positions_pc.into_iter().enumerate() {
                    source_rows += 1;
                    if position.iter().all(|p| p.is_finite()) {
                        rows.push(Object {
                            position: position.map(f64::from),
                            radius_pc: 0.,
                            row: i + 1,
                            group: group as u8,
                            name: None,
                        });
                    }
                }
            }
        }
    }
    let radius = rows
        .iter()
        .map(|o| o.position)
        .map(|p| p[0].hypot(p[1]).hypot(p[2]))
        .fold(1_f64, f64::max);
    Ok(Objects {
        radiation,
        rows,
        source_rows,
        radius,
    })
}

pub(super) fn sdss_class(eclass: f32) -> u8 {
    (((eclass + 0.35) / (0.5_f32 + 0.35) * 5.) as u8).min(4)
}
pub(super) fn nearby_radius(distance: f32, diameter: f32) -> f64 {
    let angle = f64::from(diameter) * std::f64::consts::PI / 21600.;
    if angle.is_finite()
        && angle > 0.
        && angle < std::f64::consts::FRAC_PI_2
        && distance.is_finite()
        && distance > 0.
    {
        f64::from(distance) * angle
    } else {
        0.
    }
}
