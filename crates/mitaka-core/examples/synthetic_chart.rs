//! 配布物の起動検証用。測定データは使用しない。
use std::{env, fs, io, path::PathBuf};
fn main() -> io::Result<()> {
    let root = PathBuf::from(
        env::args_os()
            .nth(1)
            .ok_or_else(|| io::Error::other("新規ディレクトリを指定してください"))?,
    );
    fs::create_dir(&root)?;
    let mut hip = Vec::new();
    for i in 1..=4096i32 {
        hip.extend(i.to_le_bytes());
        let lon = (i * 137 % 360) as f32 + (i % 113) as f32 / 127.;
        let lat = (i * 23 % 171 - 85) as f32;
        for n in [
            lon,
            lat,
            20.,
            0.,
            0.,
            0.,
            (i % 55) as f32 / 10.,
            (i % 18) as f32 / 10.,
        ] {
            hip.extend(n.to_le_bytes());
        }
        hip.extend([b'G', 2, 0, 0, 0, 0, 0, 0, 0]);
    }
    fs::write(root.join("hip.dat"), hip)?;
    let mut cf = b"CF96".to_vec();
    cf.extend(9i32.to_le_bytes());
    for planet in 1..=9 {
        cf.extend(2451000f64.to_le_bytes());
        cf.extend(2000f64.to_le_bytes());
        for n in [0i32, 1, 1, 0] {
            cf.extend(n.to_le_bytes());
        }
        for axis in 0..3 {
            cf.extend(2i32.to_le_bytes());
            let angle = f64::from(planet) * 1.78;
            let pos = [angle.cos(), angle.sin(), 0.1][axis] * f64::from(planet);
            cf.extend((pos * 1e10).to_le_bytes());
            cf.extend(0f64.to_le_bytes());
            cf.extend(1i32.to_le_bytes());
            cf.extend(0i32.to_le_bytes());
        }
    }
    fs::write(root.join("cf96.dat"), cf)
}
