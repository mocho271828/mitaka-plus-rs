//! 明示されたスクリプトを停止できる子プロセスとして起動する。
use command_group::{CommandGroup, GroupChild};
use std::{ffi::OsStr, io, path::Path, process::Command};

/// 返したハンドルの `kill` と `wait` で、同じプロセスグループの実行を停止・回収する。
pub fn spawn_helper(
    executable: &Path,
    script: &Path,
    host: Option<&OsStr>,
) -> io::Result<GroupChild> {
    let mut command = Command::new(executable);
    if let Some(endpoint) = host {
        command.arg("--host").arg(endpoint);
    }
    command.arg("--").arg(script).group_spawn()
}
