use mitaka_core::terrain::Terrain;
use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or("usage: terrain_probe FILE LONGITUDE LATITUDE")?,
    );
    let mut coordinate = || -> Result<f64, Box<dyn Error>> {
        Ok(args
            .next()
            .and_then(|s| s.into_string().ok())
            .ok_or("missing coordinate")?
            .parse()?)
    };
    let (lon, lat) = (coordinate()?, coordinate()?);
    let mut terrain = Terrain::open(&path)?;
    println!(
        "radius_m={} elevation_m={}",
        terrain.radius_m(),
        terrain.elevation_m(lon, lat)?
    );
    Ok(())
}
