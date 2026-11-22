use mitaka_core::catalog::load_hipparcos;
use std::{env, io, path::PathBuf};

fn main() -> io::Result<()> {
    let file = env::args_os().nth(1).map(PathBuf::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: catalog_probe path/to/hip.dat",
        )
    })?;
    let stars = load_hipparcos(&file)?;
    println!("HIP: {} records", stars.len());
    Ok(())
}
