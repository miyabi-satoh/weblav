//! テスト専用の共有ヘルパー。
//!
//! lib・bin クレートと統合テスト (`tests/api/main.rs`) から同じファイルを `#[path]` で
//! 読み込んでいる。`cfg(test)` はクレートごとに閉じており、`pub(crate)` では
//! クレート境界を越えられないため、ソースファイル自体を共有する形にしている。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// テスト用の最小限の一時ディレクトリ(外部クレートを増やさないための自前実装)。
/// `Drop` で削除するので、途中で `panic!` してもゴミが残らない。
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// `target/test-tmp/` の下に作る (→ `project_temp_dir`)。
    pub(crate) fn new(name: &str) -> Self {
        project_temp_dir(
            "test-tmp",
            &format!("{name}-{:?}", std::thread::current().id()),
        )
    }

    /// `path` に作る (無ければ作る)。置き場所を呼び出し側が決めたいとき (統合テストの
    /// 「公開できるフォルダ」の配下など) に使う。
    #[allow(dead_code)]
    pub(crate) fn at(path: PathBuf) -> Self {
        std::fs::create_dir_all(&path).expect("一時ディレクトリを作れなかった");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

/// `PathBuf` を返していた頃の呼び出し側 (`dir.join(...)`・`create_dir_all(&dir)`) をそのまま通すため。
impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// テスト用の一時ディレクトリ。`CARGO_MANIFEST_DIR` 配下の `target/` に作る。
/// `subdir` は呼び出し元(ファイル)ごとの名前空間。
///
/// `std::env::temp_dir()` を使わないのは、片付け漏れが普段見ない場所 (macOSなら
/// `/var/folders/...`) に溜まり続けて気づけないため。
pub(crate) fn project_temp_dir(subdir: &str, name: &str) -> TempDir {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(subdir)
        .join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("一時ディレクトリを作れなかった");
    TempDir(std::fs::canonicalize(&dir).expect("一時ディレクトリをcanonicalizeできなかった"))
}

/// 誰も待ち受けていないループバックのアドレス。bind してすぐ捨てるので、他プロセスに
/// 横取りされる小さな競合の余地はあるが、テスト用途では十分。
pub(crate) fn unused_loopback_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("ループバックに bind できなかった")
        .local_addr()
        .expect("bind したアドレスを取れなかった")
    // listener はここで drop され、そのポートでは誰も待ち受けていない。
}
