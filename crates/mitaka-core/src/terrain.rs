//! Earth/Mars topography, from Mitaka 1.3.1 mtk_topography.cpp and Mitaka_plus f9fe624.
//! Copyright (c) 2007-2014 Tsunehiko Kato. See licenses/Mitaka.txt.
use std::{
    fs::{File, Metadata, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    time::SystemTime,
};

const MAX_PATCHES: usize = 65_536;
const MAX_INDEX: usize = 32 << 20;
const MAX_FILE: u64 = 2 << 30;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn word<R: Read>(input: &mut R) -> io::Result<[u8; 4]> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn integer<R: Read>(input: &mut R) -> io::Result<i32> {
    Ok(i32::from_le_bytes(word(input)?))
}

fn number<R: Read>(input: &mut R) -> io::Result<f64> {
    let value = f32::from_le_bytes(word(input)?);
    if !value.is_finite() {
        return Err(invalid("non-finite terrain field"));
    }
    Ok(f64::from(value))
}

#[derive(Clone, Debug, PartialEq)]
struct Revision {
    bytes: u64,
    modified: SystemTime,
    #[cfg(unix)]
    change_time: (i64, i64),
}

impl Revision {
    fn from(metadata: Metadata) -> io::Result<Self> {
        if !metadata.is_file() || metadata.len() > MAX_FILE {
            return Err(invalid(
                "terrain source must be a regular file within the size limit",
            ));
        }
        Ok(Self {
            bytes: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            change_time: {
                use std::os::unix::fs::MetadataExt;
                (metadata.ctime(), metadata.ctime_nsec())
            },
        })
    }
}

#[derive(Clone, Debug)]
struct Grid {
    offset: u64,
    columns: usize,
    rows: usize,
}
impl Grid {
    fn len(&self) -> usize {
        (self.columns + 3) * (self.rows + 3) * 2
    }
}

#[derive(Debug)]
struct Patch {
    west: f64,
    east: f64,
    north: f64,
    south: f64,
    grids: std::ops::Range<usize>,
}

pub struct Terrain {
    file: File,
    revision: Revision,
    unit: f64,
    radius: f64,
    patches: Vec<Patch>,
    grids: Vec<Grid>,
    cached: Option<(usize, Vec<u8>)>,
}

impl Terrain {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let mut file = options.open(path)?;
        let revision = Revision::from(file.metadata()?)?;
        let unit = number(&mut file)?;
        let radius = number(&mut file)?;
        let cols = integer(&mut file)?;
        let rows = integer(&mut file)?;
        let count = i64::from(cols) * i64::from(rows);
        if unit <= 0.0 || radius <= 0.0 || cols <= 0 || rows <= 0 || count > MAX_PATCHES as i64 {
            return Err(invalid("invalid terrain sphere header"));
        }
        let mut patches = Vec::with_capacity(count as usize);
        let mut grids = Vec::new();
        for _ in 0..count {
            let west = number(&mut file)?;
            let east = number(&mut file)?;
            let north = number(&mut file)?;
            let south = number(&mut file)?;
            let last = integer(&mut file)?;
            if west >= east
                || east - west > 360.0
                || north <= south
                || south < -90.0
                || north > 90.0
                || !(0..10).contains(&last)
            {
                return Err(invalid("invalid terrain patch bounds or levels"));
            }
            let first = grids.len();
            for _ in 0..=last {
                // ファイル内の順序がLOD。宣言された番号の連続性は仮定しない。
                let _stored_level = integer(&mut file)?;
                let columns = integer(&mut file)?;
                let rows = integer(&mut file)?;
                if !(1..=256).contains(&columns) || !(1..=256).contains(&rows) {
                    return Err(invalid("invalid terrain grid dimensions"));
                }
                let grid = Grid {
                    offset: file.stream_position()?,
                    columns: columns as usize,
                    rows: rows as usize,
                };
                let end = grid.offset + grid.len() as u64;
                if end > revision.bytes {
                    return Err(invalid("truncated terrain payload"));
                }
                file.seek(SeekFrom::Start(end))?;
                grids.push(grid);
                if grids.capacity() * size_of::<Grid>() + patches.capacity() * size_of::<Patch>()
                    > MAX_INDEX
                {
                    return Err(invalid("terrain index too large"));
                }
            }
            patches.push(Patch {
                west,
                east,
                north,
                south,
                grids: first..grids.len(),
            });
        }
        if file.stream_position()? != revision.bytes
            || Revision::from(file.metadata()?)? != revision
        {
            return Err(invalid(
                "terrain contains extra bytes or changed during indexing",
            ));
        }
        Ok(Self {
            file,
            revision,
            unit,
            radius,
            patches,
            grids,
            cached: None,
        })
    }

    pub fn radius_m(&self) -> f64 {
        self.radius
    }

    /// 最高解像度の格子を使う双線形標高。画面のLODを使う着地交点とは異なる。
    pub fn elevation_m(&mut self, longitude_deg: f64, latitude_deg: f64) -> io::Result<f64> {
        if !longitude_deg.is_finite() || !(-90.0..=90.0).contains(&latitude_deg) {
            return Err(invalid("invalid terrain location"));
        }
        if Revision::from(self.file.metadata()?)? != self.revision {
            self.cached = None;
            return Err(invalid("terrain source changed"));
        }
        let (patch, lon) = self
            .patches
            .iter()
            .find_map(|p| {
                let lon = if (p.west..=p.east).contains(&longitude_deg) {
                    longitude_deg
                } else {
                    p.west
                        + (longitude_deg.rem_euclid(360.0) - p.west.rem_euclid(360.0))
                            .rem_euclid(360.0)
                };
                ((p.south..=p.north).contains(&latitude_deg) && lon <= p.east).then_some((p, lon))
            })
            .ok_or_else(|| invalid("location outside terrain patches"))?;
        let index = patch.grids.end - 1;
        let grid = &self.grids[index];
        let x = ((lon - patch.west) / (patch.east - patch.west)) * grid.columns as f64;
        let y = ((patch.north - latitude_deg) / (patch.north - patch.south)) * grid.rows as f64;
        if self.cached.as_ref().map(|entry| entry.0) != Some(index) {
            self.cached = None;
            self.file.seek(SeekFrom::Start(grid.offset))?;
            let mut bytes = vec![0u8; grid.len()];
            self.file.read_exact(&mut bytes)?;
            if Revision::from(self.file.metadata()?)? != self.revision {
                return Err(invalid("terrain changed while reading grid"));
            }
            self.cached = Some((index, bytes));
        }
        let bytes = &self
            .cached
            .as_ref()
            .expect("grid installed after successful read")
            .1;
        let column = (x as usize).min(grid.columns - 1);
        let row = (y as usize).min(grid.rows - 1);
        let sample = |dx: usize, dy: usize| {
            let i = 2 * ((row + 1 + dy) * (grid.columns + 3) + column + 1 + dx);
            f64::from(i16::from_le_bytes([bytes[i], bytes[i + 1]]))
        };
        let dx = x - column as f64;
        let dy = y - row as f64;
        let top = (1.0 - dx) * sample(0, 0) + dx * sample(1, 0);
        let bottom = (1.0 - dx) * sample(0, 1) + dx * sample(1, 1);
        Ok(self.unit * ((1.0 - dy) * top + dy * bottom))
    }
}
