//! Read a user-selected Mitaka data directory; always leaves the input untouched.
use mitaka_core::{
    catalog::{load_constellations, load_hipparcos},
    ephemeris::Cf96Ephemeris,
    orbit::mean_planet_state,
};
use std::{env, io, path::PathBuf};
fn main() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: core_probe DATA_DIR [JD]",
        )
    })?);
    let jd = match args.next() {
        Some(v) => v
            .to_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "expected finite Julian date")
            })?,
        None => 2451545.,
    };
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many arguments",
        ));
    }
    let stars = load_hipparcos(&root.join("hip.dat"))?;
    let constellations = load_constellations(&root.join("constellation_HIP.dat"))?;
    let eph = Cf96Ephemeris::load(&root.join("cf96.dat"))?;
    println!(
        "HIP={} constellations={} CF96={} JD={jd}",
        stars.len(),
        constellations.len(),
        eph.planet_count()
    );
    println!("CF96 J2000-equatorial AU AU/day");
    for i in 0..eph.planet_count() {
        let state = eph.state(i, jd)?;
        print!("{i}");
        for v in state
            .position_au
            .into_iter()
            .chain(state.velocity_au_per_day)
        {
            print!(" {v:.17e}");
        }
        println!();
    }
    println!("Mean planets ecliptic AU");
    for i in 0..9 {
        let [x, y, z] = mean_planet_state(i, jd)?.position_au;
        println!("{i} {x:.17e} {y:.17e} {z:.17e}");
    }
    Ok(())
}
