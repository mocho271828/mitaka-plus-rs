//! f9fe624 media_catalog.mm の登録行・重複・文字コード・MVP選択を保つ。
//! 読取で得た候補に再生権限は付与しない。GUIが選択した対象のFileだけを保持する。
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Image,
    Movie,
    Audio,
    Unknown,
}
impl Kind {
    pub fn of(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "jpg" | "jpeg" | "png" => Self::Image,
            "mov" | "m4v" | "mp4" | "avi" | "webm" | "wmv" | "mpg" | "mpeg" | "ogv" => Self::Movie,
            "mp3" | "wav" | "ogg" | "oga" | "m4a" | "aac" | "flac" | "aif" | "aiff" | "caf"
            | "wma" => Self::Audio,
            _ => Self::Unknown,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "画像",
            Self::Movie => "動画",
            Self::Audio => "音声",
            Self::Unknown => "不明",
        }
    }
}
#[derive(Debug)]
pub struct Grant {
    pub file: Arc<File>,
    pub path: PathBuf,
}
impl Grant {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::from_file(path, regular(path)?)
    }
    fn from_file(path: &Path, file: File) -> io::Result<Self> {
        if file.metadata()?.len() == 0 || Kind::of(path) == Kind::Unknown {
            return Err(io::Error::other("空または未対応の媒体です"));
        }
        Ok(Self {
            file: Arc::new(file),
            path: path.to_owned(),
        })
    }
}
fn regular(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("通常ファイルを選択してください"));
    }
    Ok(file)
}
struct TrustedRoot {
    path: PathBuf,
    dir: cap_std::fs::Dir,
}
impl TrustedRoot {
    fn open(&self, path: &Path) -> io::Result<File> {
        let relative = path.strip_prefix(&self.path).map_err(io::Error::other)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = self.dir.open_with(relative, &options)?.into_std();
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("通常ファイルではありません"));
        }
        Ok(file)
    }
}
fn text(root: &TrustedRoot, path: &Path, limit: usize) -> io::Result<String> {
    let file = root.open(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(io::Error::other("登録ファイルの上限を超えました"));
    }
    let mut bytes = vec![];
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit || bytes.contains(&0) {
        return Err(io::Error::other("不正なサイズまたはNULを含む登録です"));
    }
    let text = if let Ok(s) = std::str::from_utf8(&bytes) {
        s.to_owned()
    } else {
        let (s, error) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&bytes);
        if error {
            return Err(io::Error::other("UTF-8/CP932ではありません"));
        }
        s.into_owned()
    };
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}
fn relative(root: &Path, start: &Path, raw: &str) -> io::Result<PathBuf> {
    let raw = raw.replace('\\', "/");
    let path = Path::new(&raw);
    if raw.is_empty() || raw.contains(':') || path.is_absolute() {
        return Err(io::Error::other("絶対パス・URLは手動で指定してください"));
    }
    let mut result = start.canonicalize()?;
    if !result.starts_with(root) {
        return Err(io::Error::other("選択したデータ領域の外部です"));
    }
    for component in path.components() {
        let name = match component {
            Component::CurDir => continue,
            Component::Normal(s) => s
                .to_str()
                .ok_or_else(|| io::Error::other("不正なファイル名"))?,
            _ => {
                return Err(io::Error::other(
                    "親フォルダへの参照は手動で指定してください",
                ))
            }
        };
        let mut choices = vec![];
        for e in fs::read_dir(&result)? {
            let e = e?;
            if e.file_name()
                .to_str()
                .is_some_and(|value| value.nfc().eq(name.nfc()))
            {
                choices.push(e.path());
            }
        }
        if choices.len() != 1 {
            return Err(io::Error::other(
                "登録と一致するファイルを一意に特定できません",
            ));
        }
        result = choices.pop().unwrap().canonicalize()?;
        if !result.starts_with(root) || !result.starts_with(start) {
            return Err(io::Error::other("登録フォルダ外への参照です"));
        }
    }
    if !result.is_file() {
        return Err(io::Error::other("通常ファイルではありません"));
    }
    Ok(result)
}
#[derive(Debug)]
pub struct Entry {
    pub kind: Kind,
    pub name: String,
    pub title: String,
    pub source: String,
    pub descriptor: String,
    pub secondary: String,
    pub paths: Vec<String>,
    pub resolved: Option<PathBuf>,
    pub grant: Option<Grant>,
    pub issue: String,
}
impl Entry {
    pub fn effective(&self) -> Kind {
        self.grant.as_ref().map_or(self.kind, |g| Kind::of(&g.path))
    }
    pub fn direct(grant: Grant) -> Self {
        let name = grant
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        Self {
            kind: Kind::of(&grant.path),
            name: name.clone(),
            title: name,
            source: "手動選択".into(),
            descriptor: String::new(),
            secondary: String::new(),
            paths: vec![],
            resolved: None,
            issue: String::new(),
            grant: Some(grant),
        }
    }
}
#[derive(Default)]
pub struct Registry {
    root: Option<TrustedRoot>,
    pub entries: Vec<Entry>,
    pub issues: Vec<String>,
}
impl Registry {
    pub fn load(data: &Path) -> Self {
        let mut out = Self::default();
        let Some(root) = data.parent().and_then(|p| p.canonicalize().ok()) else {
            return out;
        };
        let directory =
            match cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority()) {
                Ok(d) => d,
                Err(e) => {
                    out.issues.push(e.to_string());
                    return out;
                }
            };
        let trusted = TrustedRoot {
            path: root.clone(),
            dir: directory,
        };
        let dir = root.join("media");
        let mut primary = HashMap::new();
        for name in ["images.dat", "images_list.dat", "movies.dat", "audio.dat"] {
            let source = match relative(&root, &dir, name) {
                Ok(p) => p,
                Err(_) if !dir.join(name).exists() => continue,
                Err(e) => {
                    out.issues.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let data = match text(&trusted, &source, 1024 * 1024) {
                Ok(s) => s,
                Err(e) => {
                    out.issues.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let mut seen = HashMap::<String, usize>::new();
            for (n, raw) in data.lines().enumerate() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let count = seen.entry(line.to_owned()).or_default();
                *count += 1;
                if name == "images_list.dat" && *count <= *primary.get(line).unwrap_or(&0) {
                    continue;
                }
                let fields = line.split_whitespace().collect::<Vec<_>>();
                let mut entry = Entry {
                    kind: Kind::Unknown,
                    name: line.into(),
                    title: String::new(),
                    source: format!("{name}:{}", n + 1),
                    descriptor: String::new(),
                    secondary: String::new(),
                    paths: vec![],
                    resolved: None,
                    grant: None,
                    issue: String::new(),
                };
                if let [descriptor, secondary, kind, title, display] = fields.as_slice() {
                    entry.kind = match *kind {
                        "I" => Kind::Image,
                        "M" => Kind::Movie,
                        "A" => Kind::Audio,
                        _ => Kind::Unknown,
                    };
                    entry.descriptor = (*descriptor).into();
                    entry.secondary = (*secondary).into();
                    entry.name = (*title).into();
                    entry.title = (*display).into();
                    let load = || -> io::Result<Vec<String>> {
                        let descriptor = relative(&root, &dir, descriptor)?;
                        let list = text(&trusted, &descriptor, 64 * 1024)?
                            .lines()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .collect::<Vec<_>>();
                        if list.is_empty() {
                            return Err(io::Error::other("MVPが空です"));
                        }
                        Ok(list)
                    };
                    match load() {
                        Ok(paths) => {
                            entry.paths = paths;
                            match relative(&root, &root, entry.paths.last().unwrap()) {
                                Ok(path) => entry.resolved = Some(path),
                                Err(e) => entry.issue = e.to_string(),
                            }
                        }
                        Err(e) => entry.issue = e.to_string(),
                    }
                    if entry.kind == Kind::Unknown {
                        entry.issue = format!("旧type {kind} は未確定です。 {}", entry.issue);
                    }
                } else {
                    entry.issue = "登録は5列必要です".into();
                }
                out.entries.push(entry);
            }
            if name == "images.dat" {
                primary = seen;
            } else if name == "images_list.dat"
                && !primary.is_empty()
                && !seen.is_empty()
                && primary != seen
            {
                out.issues.push("2つの画像登録に異なる行があります".into());
            }
        }
        out.root = Some(trusted);
        out
    }
    pub fn candidate_grant(&self, idx: usize) -> io::Result<Grant> {
        let path = self.entries[idx]
            .resolved
            .as_ref()
            .ok_or_else(|| io::Error::other("登録ファイルを手動で指定してください"))?;
        let root = self
            .root
            .as_ref()
            .ok_or_else(|| io::Error::other("登録ルートがありません"))?;
        Grant::from_file(path, root.open(path)?)
    }
    pub fn indices(&self, kind: Kind) -> impl Iterator<Item = usize> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter_map(move |(i, e)| (e.effective() == kind).then_some(i))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_then_grant_is_explicit() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        let d = r.join("data");
        let m = r.join("media");
        fs::create_dir(&m).unwrap();
        fs::create_dir(&d).unwrap();
        fs::write(
            m.join("images.dat"),
            encoding_rs::SHIFT_JIS
                .encode("x.mvp x.mvp I 日本 {日|にち}\r\n")
                .0,
        )
        .unwrap();
        fs::write(
            m.join("images_list.dat"),
            "x.mvp x.mvp I 日本 {日|にち}\nx.mvp x.mvp P 未知 登録\n",
        )
        .unwrap();
        fs::write(m.join("x.mvp"), "..\\no.jpg\n.\\日本.jpg\n").unwrap();
        fs::write(r.join("日本.jpg"), b"first").unwrap();
        let entries = Registry::load(&d).entries;
        assert_eq!(entries.len(), 2);
        assert!(entries[0].grant.is_none());
        assert_eq!(
            entries[0].resolved.as_ref().unwrap().file_name().unwrap(),
            "日本.jpg"
        );
        assert_eq!(entries[1].kind, Kind::Unknown);
        assert!(relative(r, r, "../escape").is_err());
        assert!(relative(r, r, "https://example.org/").is_err());
    }
}

#[cfg(test)]
mod real {
    #[test]
    #[ignore = "requires a local Plus 1.5.4 data directory"]
    fn native_registration_baseline() {
        let root = std::env::var_os("MITAKA_DATA_ROOT").unwrap();
        let catalog = super::Registry::load(std::path::Path::new(&root));
        println!(
            "{} registrations, {} resolved, {} granted",
            catalog.entries.len(),
            catalog
                .entries
                .iter()
                .filter(|e| e.resolved.is_some())
                .count(),
            catalog.entries.iter().filter(|e| e.grant.is_some()).count()
        );
        assert_eq!(catalog.entries.len(), 9);
        assert_eq!(
            catalog
                .entries
                .iter()
                .filter(|e| e.kind == super::Kind::Unknown)
                .count(),
            2
        );
        assert!(catalog.entries.iter().all(|e| e.grant.is_none()));
        assert_eq!(catalog.entries[8].name, "オリオン星雲");
    }
}

#[cfg(all(test, unix))]
mod replacement {
    use super::*;
    #[test]
    fn displayed_candidate_cannot_redirect_outside_after_registration() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let media = root.join("media");
        fs::create_dir_all(&media).unwrap();
        fs::create_dir(root.join("data")).unwrap();
        fs::write(media.join("images.dat"), "x.mvp x.mvp I 許可候補 名称").unwrap();
        fs::write(media.join("x.mvp"), "x.png").unwrap();
        fs::write(root.join("x.png"), "candidate").unwrap();
        let r = Registry::load(&root.join("data"));
        let old = r.candidate_grant(0).unwrap();
        fs::write(dir.path().join("private.png"), "unselected").unwrap();
        fs::remove_file(root.join("x.png")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("private.png"), root.join("x.png")).unwrap();
        assert!(r.candidate_grant(0).is_err());
        let mut content = String::new();
        old.file
            .try_clone()
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "candidate");
    }
}
