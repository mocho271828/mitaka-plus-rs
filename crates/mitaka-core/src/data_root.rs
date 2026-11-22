use std::io;
use std::path::{Path, PathBuf};

/// GUI で選択する data ディレクトリ。呼び出し側のカレントディレクトリに依存しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot(PathBuf);

impl DataRoot {
    pub fn open(path: &Path) -> io::Result<Self> {
        let root = path.canonicalize()?;
        for name in ["hip.dat", "cf96.dat"] {
            let file = root.join(name);
            if !file
                .metadata()
                .map_err(|err| io::Error::new(err.kind(), format!("{}: {err}", file.display())))?
                .is_file()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{}: 通常ファイルを選択してください", file.display()),
                ));
            }
        }
        Ok(Self(root))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_a_missing_directory() {
        assert_eq!(
            DataRoot::open(Path::new("absent-fixture/hip.dat"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }
}
