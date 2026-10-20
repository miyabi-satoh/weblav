//! 公開できるフォルダーが今読めるかを、時間で打ち切って確かめる (→ docs/folders.md「NAS と外付けのディスク」)。
//!
//! NAS が落ちていると、その共有を触った所でファイル操作が長く戻らない。配下を触る前にここを通し、
//! 読めないフォルダーは待たずに外す。確かめる操作は、戻らなくても要求を待たせ続けないよう、
//! 決めた時間で待つのをやめる (操作そのものは止められないので、裏で戻るまで走らせておく)。
//! 同じフォルダーへの確かめは1本にまとめ、戻らないものが重なってスレッドを食い潰さないようにする。
//!
//! 共有やディスクごと外れているときは、OS につなぎ直しを頼んでから確かめ直す
//! (macOS はブックマークの解決、Ubuntu は `gio mount`。Windows の UNC パスは OS が自分でつなぐ)。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use tokio::sync::watch;

/// フォルダーの今の様子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// 読める。中身は canonicalize 済みのパス。
    Ready(PathBuf),
    /// 共有やディスクはあるが、フォルダーが無い・読めない。登録したまま消されたフォルダーと同じに扱う。
    Gone,
    /// 共有やディスクにつながらない (外れている・NAS が落ちている・時間内に返事が無い)。
    Unreachable,
}

/// 開こうとしているフォルダーを待つ時間。眠っている NAS やディスクの HDD が回り出すのに10秒ほど掛かるので、
/// それを越えて待つ。
pub const TARGET_WAIT: Duration = Duration::from_secs(20);

/// 一覧や検索で、開こうとしていないフォルダーを待つ時間。ほかのフォルダーのために、手元のフォルダーの要求を
/// 待たせないための短さ。間に合わなかったフォルダーは、その要求では外れているものとして扱う。
pub const OTHERS_WAIT: Duration = Duration::from_secs(1);

/// 読めると分かった結果を使い回す間。一覧の縮小画像のように、短い間に同じフォルダーへ要求が重なるため。
/// 長くすると、NAS が落ちた直後の要求が確かめを通らずに止まる。
const READY_FRESH: Duration = Duration::from_secs(2);

/// 読めないと分かった結果を使い回す間。つながらないフォルダーへの確かめを、要求ごとに繰り返さないため。
const FAILED_FRESH: Duration = Duration::from_secs(5);

/// つなぎ直しを頼む間隔。つながらない NAS へ、要求ごとにマウントを試みないため。
const RECONNECT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct FolderReach {
    entries: Mutex<HashMap<String, Entry>>,
}

#[derive(Default)]
struct Entry {
    checked: Option<(Instant, Reach)>,
    /// 走っている確かめと、始めた時刻。終わると `Some` が入る。
    running: Option<(Instant, watch::Receiver<Option<Reach>>)>,
    reconnected_at: Option<Instant>,
}

impl FolderReach {
    /// `path` (登録した公開できるフォルダー) の様子。`wait` を過ぎても確かめが終わらなければ `Unreachable`。
    pub async fn check(self: &Arc<Self>, pool: &SqlitePool, path: &str, wait: Duration) -> Reach {
        let (mut receiver, wait) = match self.begin(path) {
            Begin::Known(reach) => return reach,
            // 戻らないままの確かめに、後から来た要求がそれぞれ `TARGET_WAIT` ずつ待たされないよう、
            // 始めた時刻から数えて打ち切る。
            Begin::Running(started, receiver) => (
                receiver,
                wait.min(TARGET_WAIT.saturating_sub(started.elapsed())),
            ),
            Begin::Start { reconnect } => {
                // つなぎ直しに使うブックマークは、確かめを始めるときだけ読む (macOS 以外は行が無い)。
                let bookmark = if reconnect {
                    crate::folder_access::load(pool, path).await
                } else {
                    None
                };
                (self.start(path, reconnect, bookmark), wait)
            }
        };
        let finished = tokio::time::timeout(wait, receiver.wait_for(Option::is_some)).await;
        match finished {
            Ok(Ok(reach)) => reach.clone().unwrap_or(Reach::Unreachable),
            // 待ちきれなかった・確かめのタスクが落ちた。
            _ => Reach::Unreachable,
        }
    }

    fn begin(&self, path: &str) -> Begin {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries.entry(path.to_string()).or_default();
        if let Some((at, reach)) = &entry.checked {
            let fresh = match reach {
                Reach::Ready(_) => READY_FRESH,
                Reach::Gone | Reach::Unreachable => FAILED_FRESH,
            };
            if at.elapsed() < fresh {
                return Begin::Known(reach.clone());
            }
        }
        if let Some((started, receiver)) = &entry.running {
            return Begin::Running(*started, receiver.clone());
        }
        let reconnect = entry
            .reconnected_at
            .is_none_or(|at| at.elapsed() >= RECONNECT_INTERVAL);
        Begin::Start { reconnect }
    }

    fn start(
        self: &Arc<Self>,
        path: &str,
        reconnect: bool,
        bookmark: Option<Vec<u8>>,
    ) -> watch::Receiver<Option<Reach>> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries.entry(path.to_string()).or_default();
        // ブックマークを読んでいる間に、ほかの要求が始めていればそれを待つ。
        if let Some((_, receiver)) = &entry.running {
            return receiver.clone();
        }
        let (sender, receiver) = watch::channel(None);
        entry.running = Some((Instant::now(), receiver.clone()));
        drop(entries);

        let this = Arc::clone(self);
        let path = path.to_string();
        tokio::spawn(async move {
            let probe_path = path.clone();
            let result =
                tokio::task::spawn_blocking(move || probe(&probe_path, reconnect, bookmark)).await;
            let (reach, reconnected) = match result {
                Ok(result) => result,
                Err(err) => {
                    tracing::warn!(%path, %err, "folder check failed");
                    (Reach::Unreachable, false)
                }
            };
            if reach == Reach::Unreachable {
                tracing::info!(%path, "a shared folder is not reachable");
            }
            {
                let mut entries = this.entries.lock().unwrap_or_else(PoisonError::into_inner);
                let entry = entries.entry(path).or_default();
                entry.checked = Some((Instant::now(), reach.clone()));
                entry.running = None;
                if reconnected {
                    entry.reconnected_at = Some(Instant::now());
                }
            }
            // 待っている要求がもう無ければ、送れなくてよい。
            let _ = sender.send(Some(reach));
        });
        receiver
    }

    /// 登録や削除で、覚えている様子が古くなったフォルダーを忘れる。
    pub fn forget(&self, path: &str) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(path);
    }
}

enum Begin {
    Known(Reach),
    Running(Instant, watch::Receiver<Option<Reach>>),
    Start { reconnect: bool },
}

/// 確かめる。2つ目の値は、つなぎ直しを頼んだか。
fn probe(path: &str, reconnect: bool, bookmark: Option<Vec<u8>>) -> (Reach, bool) {
    if let Ok(canonical) = readable(Path::new(path)) {
        return (Reach::Ready(canonical), false);
    }
    let Some(volume) = volume_root(path) else {
        return (Reach::Gone, false);
    };
    // 共有やディスクはつながっていて、フォルダーだけが無い。
    if std::fs::metadata(&volume).is_ok() {
        return (Reach::Gone, false);
    }
    if !reconnect {
        return (Reach::Unreachable, false);
    }
    if let Err(err) = reconnect_volume(path, bookmark.as_deref()) {
        tracing::info!(%path, %err, "failed to reconnect a shared folder");
        return (Reach::Unreachable, true);
    }
    match readable(Path::new(path)) {
        Ok(canonical) => (Reach::Ready(canonical), true),
        Err(_) => (Reach::Unreachable, true),
    }
}

/// canonicalize して、中を1件読む。マウントの一覧に残ったまま NAS が落ちていると、
/// フォルダーの属性はキャッシュから返っても、中を読む所で止まるため。
fn readable(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    std::fs::read_dir(&canonical)?.next().transpose()?;
    Ok(canonical)
}

/// 外れることのある共有やディスクの根元。外れていると、ここごと無くなる。
/// 当たらないもの (システムのディスクの中) は `None`。
///
/// - macOS: `/Volumes/<名前>` (外付けのディスク・マウントした共有)
/// - Ubuntu: `/run/user/<uid>/gvfs/<共有>` (ファイルで開いた共有)・`/media/<ユーザー>/<名前>` (外付けのディスク)
/// - Windows: UNC パスの `\\<サーバー>\<共有>`・ドライブ (`E:\`)
fn volume_root(path: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        return windows_volume_root(path);
    }
    let parts: Vec<&str> = path.split('/').collect();
    let depth = match parts.as_slice() {
        ["", "Volumes", name, ..] if !name.is_empty() => 3,
        ["", "run", "user", _, "gvfs", share, ..] if !share.is_empty() => 6,
        ["", "media", _, name, ..] if !name.is_empty() => 4,
        _ => return None,
    };
    Some(PathBuf::from(parts[..depth].join("/")))
}

fn windows_volume_root(path: &str) -> Option<PathBuf> {
    use std::path::{Component, Prefix};

    let path = Path::new(path);
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    // システムのドライブも当てるが、ドライブがある限り「フォルダーだけが無い」になる。
    match prefix.kind() {
        Prefix::UNC(..) | Prefix::VerbatimUNC(..) | Prefix::Disk(_) | Prefix::VerbatimDisk(_) => {}
        _ => return None,
    }
    let mut root = PathBuf::from(prefix.as_os_str());
    root.push(std::path::MAIN_SEPARATOR_STR);
    Some(root)
}

/// OS に共有をつなぎ直してもらう。パスワードは OS が保存したもの (キーチェーンなど) を使い、窓は出さない。
fn reconnect_volume(path: &str, bookmark: Option<&[u8]>) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let bookmark = bookmark.ok_or("no bookmark to reconnect with")?;
        return crate::folder_access::reconnect(bookmark);
    }
    if cfg!(target_os = "linux") {
        let url = gvfs_smb_url(path).ok_or("not a network share opened by Files")?;
        return gio_mount(&url);
    }
    // Windows は、UNC パスを開くときに OS がつなぐ。
    Err("reconnecting is left to the OS".to_string())
}

/// `/run/user/<uid>/gvfs/smb-share:server=<サーバー>,share=<共有>[,user=<ユーザー>]/...` の共有の URL。
/// gvfs が付ける名前の形で、`gio mount` に渡せる。
fn gvfs_smb_url(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    let ["", "run", "user", _, "gvfs", mount, ..] = parts.as_slice() else {
        return None;
    };
    let params = mount.strip_prefix("smb-share:")?;
    let mut server = None;
    let mut share = None;
    let mut user = None;
    for param in params.split(',') {
        match param.split_once('=') {
            Some(("server", value)) => server = Some(value),
            Some(("share", value)) => share = Some(value),
            Some(("user", value)) => user = Some(value),
            _ => {}
        }
    }
    let user = user.map(|user| format!("{user}@")).unwrap_or_default();
    Some(format!("smb://{user}{}/{}", server?, share?))
}

/// `gio mount` が戻るのを待つ上限。つながらないサーバーでは、gio 自身があきらめるまで戻らないため。
const GIO_MOUNT_TIMEOUT: Duration = Duration::from_secs(30);

fn gio_mount(url: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};

    // パスワードを聞かれたら、入力が無いので失敗させる (保存していない共有はつなげない)。
    let mut child = Command::new("gio")
        .args(["mount", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to run gio: {err}"))?;
    let deadline = Instant::now() + GIO_MOUNT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("gio mount exited with {status}")),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("gio mount timed out".to_string());
            }
            Err(err) => return Err(format!("failed to wait for gio: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn the_volume_root_is_the_mounted_share_or_disk() {
        assert_eq!(
            volume_root("/Volumes/share/Backup/Pictures"),
            Some(PathBuf::from("/Volumes/share"))
        );
        assert_eq!(
            volume_root("/run/user/1000/gvfs/smb-share:server=nas.local,share=media/Movies"),
            Some(PathBuf::from(
                "/run/user/1000/gvfs/smb-share:server=nas.local,share=media"
            ))
        );
        assert_eq!(
            volume_root("/media/taro/USB/photos"),
            Some(PathBuf::from("/media/taro/USB"))
        );
        assert_eq!(volume_root("/Users/taro/Pictures"), None);
        assert_eq!(volume_root("/Volumes"), None);
    }

    #[test]
    #[cfg(windows)]
    fn the_volume_root_is_the_share_or_a_removable_drive() {
        assert_eq!(
            volume_root(r"\\nas\share\Movies"),
            Some(PathBuf::from(r"\\nas\share\"))
        );
        assert_eq!(volume_root(r"E:\photos"), Some(PathBuf::from(r"E:\")));
    }

    #[test]
    fn a_gvfs_share_becomes_the_url_gio_mounts() {
        assert_eq!(
            gvfs_smb_url("/run/user/1000/gvfs/smb-share:server=nas.local,share=media/Movies")
                .as_deref(),
            Some("smb://nas.local/media")
        );
        assert_eq!(
            gvfs_smb_url(
                "/run/user/1000/gvfs/smb-share:server=192.168.1.210,share=share,user=taro"
            )
            .as_deref(),
            Some("smb://taro@192.168.1.210/share")
        );
        assert_eq!(
            gvfs_smb_url("/run/user/1000/gvfs/sftp:host=nas.local"),
            None
        );
        assert_eq!(gvfs_smb_url("/Volumes/share"), None);
    }

    #[tokio::test]
    async fn a_readable_folder_is_ready_and_a_missing_one_is_gone() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("メモリの DB を開けるはず");
        let dir = crate::test_support::TempDir::new("folder-reach");
        let reach = Arc::new(FolderReach::default());
        let path = dir.to_string_lossy().to_string();
        assert_eq!(
            reach.check(&pool, &path, TARGET_WAIT).await,
            Reach::Ready(dir.to_path_buf())
        );
        let missing = dir.join("missing").to_string_lossy().to_string();
        assert_eq!(reach.check(&pool, &missing, TARGET_WAIT).await, Reach::Gone);
    }
}
