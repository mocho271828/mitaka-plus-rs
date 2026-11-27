//! PCM WAV の標本フレーム、または等間隔の表示時刻を実表で確認できた MP4 のみフレーム変換する。
use std::{
    fs::File,
    io::{BufReader, Seek},
};
#[derive(Debug, Clone, Copy)]
pub struct Frames {
    rate: f64,
    len: u32,
}
impl Frames {
    pub fn read(file: &File, mime: &str) -> Option<Self> {
        let mut f = file.try_clone().ok()?;
        f.rewind().ok()?;
        let result = match mime {
            "audio/wav" => {
                let w = hound::WavReader::new(BufReader::new(&f)).ok()?;
                Some(Self {
                    rate: w.spec().sample_rate.into(),
                    len: w.duration(),
                })
            }
            "video/mp4" | "video/quicktime" => {
                let size = f.metadata().ok()?.len();
                if size > 256 * 1024 * 1024 {
                    return None;
                }
                let m = mp4::Mp4Reader::read_header(BufReader::new(&f), size).ok()?;
                let mut tracks = m
                    .tracks()
                    .values()
                    .filter(|t| matches!(t.track_type(), Ok(mp4::TrackType::Video)));
                let t = tracks.next()?;
                if tracks.next().is_some() || !t.trafs.is_empty() {
                    return None;
                }
                let table = &t.trak.mdia.minf.stbl;
                let len = t.sample_count();
                let scale = t.timescale();
                let [timing] = table.stts.entries.as_slice() else {
                    return None;
                };
                if timing.sample_count != len
                    || len == 0
                    || len > 1_000_000
                    || timing.sample_delta == 0
                    || scale == 0
                {
                    return None;
                }
                if let Some(list) = t.trak.edts.as_ref().and_then(|e| e.elst.as_ref()) {
                    let [edit] = list.entries.as_slice() else {
                        return None;
                    };
                    if edit.media_rate != 1 || edit.media_rate_fraction != 0 {
                        return None;
                    }
                }
                if let Some(ctts) = &table.ctts {
                    let mut times = Vec::with_capacity(len as usize);
                    for entry in &ctts.entries {
                        if entry.sample_count as usize > len as usize - times.len() {
                            return None;
                        }
                        for _ in 0..entry.sample_count {
                            times.push(
                                times.len() as i64 * i64::from(timing.sample_delta)
                                    + i64::from(entry.sample_offset),
                            );
                        }
                    }
                    times.sort_unstable();
                    if times.len() != len as usize
                        || !times
                            .windows(2)
                            .all(|w| w[1] - w[0] == i64::from(timing.sample_delta))
                    {
                        return None;
                    }
                }
                Some(Self {
                    rate: f64::from(scale) / f64::from(timing.sample_delta),
                    len,
                })
            }
            _ => None,
        };
        result.filter(|f| f.rate > 0. && f.len > 0)
    }
    pub fn verified(self, duration: f64) -> Option<(f64, u32)> {
        ((duration * self.rate - f64::from(self.len)).abs() < 0.51).then_some((self.rate, self.len))
    }
}
