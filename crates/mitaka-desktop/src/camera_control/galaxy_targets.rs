//! Galaxy.#と英名は重複を保持して、Plusのカタログから一意な対象のみを選ぶ。
use crate::catalog::SkyData;
use mitaka_core::{
    astronomy::equatorial_direction,
    galaxy::{self, evaluate_position_pc},
};
use std::io;
const AU_PC: f64 = 648000. / std::f64::consts::PI;
#[derive(Debug, Clone, Copy)]
pub(super) struct Target {
    pub pos: [f64; 3],
    pub radius: Option<f64>,
}
fn fail(s: &str) -> io::Error {
    io::Error::other(s)
}
fn location(ra: f32, dec: f32, dist: f32, radius: f64) -> io::Result<Target> {
    let p = evaluate_position_pc(ra, dec, dist)
        .position
        .ok_or_else(|| fail("距離・方向の欠測があります"))?
        .ecliptic;
    Ok(Target {
        pos: p.map(|v| v * AU_PC),
        radius: (radius.is_finite() && radius > 0.).then_some(radius * AU_PC),
    })
}
fn one<'a, T>(mut rows: impl Iterator<Item = &'a T>) -> io::Result<&'a T> {
    let first = rows.next().ok_or_else(|| fail("天体が見つかりません"))?;
    if rows.next().is_some() {
        return Err(fail("同名天体が複数あります"));
    }
    Ok(first)
}
pub(super) fn resolve(name: &str, data: &SkyData) -> io::Result<Target> {
    let name = match name {
        "Other.M31" => "Galaxy.Andromeda Galaxy",
        "Other.M87" => "Galaxy.M87",
        "Other.M13" => "GC.NGC 6205",
        n => n,
    };
    if name == "Other.SolarSystem" || name == "Other.LSS" {
        return Ok(Target {
            pos: [0.; 3],
            radius: Some(if name == "Other.LSS" {
                1e8 * AU_PC
            } else {
                50.
            }),
        });
    }
    if matches!(name, "Other.OurGalaxy" | "Other.OurGalaxyCenter") {
        // 読めない場合に「銀河」を原点に置いてしまうことを避ける。
        galaxy::OurGalaxyModel::load(&data.root.join("ourgalaxy_model.dat"))?;
        let [x, y, z] = equatorial_direction(
            (17. + 45. / 60. + 40.0383 / 3600.) * 15.,
            -(29. + 28.069 / 3600.),
        )?
        .map(|v| f64::from(v as f32));
        let (s, c) = 23.43870583_f64.to_radians().sin_cos();
        return Ok(Target {
            pos: [x, c * y + s * z, -s * y + c * z].map(|v| v * 8000. * AU_PC),
            radius: Some(15000. * AU_PC),
        });
    }
    if let Some(key) = name.strip_prefix("GC.") {
        let catalog = galaxy::load_globular_clusters(&data.root.join("globular_cluster.dat"))?;
        let r = one(catalog
            .records
            .iter()
            .filter(|r| r.name_bytes.split(|v| *v == 0).next() == Some(key.as_bytes())))?;
        return location(
            r.ra_deg,
            r.dec_deg,
            r.distance_pc,
            r.half_light_radius_pc.into(),
        );
    }
    let key = name
        .strip_prefix("Galaxy.")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail("未知の銀河ターゲットです"))?;
    let raw = key
        .strip_prefix('#')
        .and_then(|v| v.split_at_checked(1))
        .and_then(|(c, n)| {
            let id = n.parse::<i32>().ok()?;
            (c.as_bytes()[0].is_ascii_graphic() && id > 0 && id.to_string() == n)
                .then_some((c.as_bytes()[0], id))
        });
    let catalog = galaxy::load_nearby_galaxies(&data.root.join("NBG.dat"))?;
    let mut keys = Vec::new();
    if let Some(raw) = raw {
        keys.push(raw);
    }
    let table = galaxy::load_names(&data.root.join("nearby_galaxies_name.dat"));
    if let Ok(table) = table {
        for row in table.records {
            if row.kind != galaxy::NameKind::Entry {
                continue;
            }
            let end = row
                .payload
                .windows(2)
                .position(|p| p == b"  ")
                .unwrap_or(row.payload.len())
                .min(
                    row.payload
                        .iter()
                        .position(|b| !b.is_ascii())
                        .unwrap_or(row.payload.len()),
                );
            if row.payload[..end].trim_ascii() != key.as_bytes() {
                continue;
            }
            if let Some((&code, n)) = row.key.split_first() {
                if let Some(n) = std::str::from_utf8(n)
                    .ok()
                    .and_then(|v| v.parse::<i32>().ok())
                    .filter(|v| *v > 0)
                {
                    keys.push((code, n));
                }
            }
        }
    } else if keys.is_empty() {
        return Err(fail("銀河の英名表を読めません"));
    }
    let r = one(catalog
        .records
        .iter()
        .filter(|r| keys.contains(&(r.catalog_code, r.catalog_number))))?;
    let rad = (f64::from(r.diameter_arcmin) / 120.).to_radians();
    location(
        r.ra_deg,
        r.dec_deg,
        r.distance_pc,
        if rad > 0. && rad < std::f64::consts::FRAC_PI_2 {
            f64::from(r.distance_pc) * rad
        } else {
            0.
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "実データを MITAKA_DATA_ROOT に指定して実行"]
    fn shipped_galaxy_targets_are_unique_and_resolve_to_physical_positions() {
        let sky = SkyData::open(std::path::Path::new(
            &std::env::var_os("MITAKA_DATA_ROOT").unwrap(),
        ))
        .unwrap();
        for (alias, key) in [
            ("Other.M31", "Galaxy.#N224"),
            ("Other.M87", "Galaxy.#N4486"),
            ("Other.M13", "GC.NGC 6205"),
        ] {
            let a = resolve(alias, &sky).unwrap();
            let b = resolve(key, &sky).unwrap();
            assert_eq!(a.pos, b.pos);
            assert!(a.radius.is_some());
            assert!(a.pos.iter().any(|v| v.abs() > 1e6));
        }
        assert!(resolve("Galaxy.#N00224", &sky).is_err());
    }
}
