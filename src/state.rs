use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::api::setup::SetupTokens;
use crate::auth::LoginRateLimiter;

/// axumハンドラ間で共有するアプリケーション状態。
/// `SqlitePool` は内部で`Arc`を持つため、`Clone`しても実体は共有される。
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub login_rate_limiter: Arc<LoginRateLimiter>,
    /// リカバリコードでの再設定の試行を絞る (→ docs/access.md「リカバリコード」)。ログインとは別の枠。
    pub recovery_rate_limiter: Arc<LoginRateLimiter>,
    /// ファイルアップロード型コンテンツの実体(content-addressed blob)を置くディレクトリ。
    /// `AppDirs::blobs_dir()`。
    pub blobs_dir: PathBuf,
    /// 画像の縮小画像の置き場と、同時に作る数の上限 (→ `api::thumbnails`)。
    pub thumbnails: Arc<crate::api::thumbnails::Thumbnails>,
    /// リンクのカードの画像の置き場と、同時に取りに行く数の上限 (→ `api::link_preview`)。
    pub link_previews: Arc<crate::api::link_preview::LinkPreviews>,
    /// weblav 自身の置き場 (`AppDirs` の設定とデータ)。設定ファイル・DB・セッション鍵・blobが入る。
    /// ここ自身とその中は、公開できるフォルダーの中にあっても登録先・一覧・配信に使わせない
    /// (→ `api::roots::OwnDirs`、docs/folders.md「公開できるフォルダー」)。
    pub own_dirs: Vec<PathBuf>,
    /// 管理画面から書き換える設定ファイル (`AppDirs::config_path()`、→ `api::server_settings`)。
    pub config_path: PathBuf,
    /// 起動したときに読んだ、管理画面で変えられる設定。ポートは実際に待ち受けている番号。
    /// 書き換えても次の起動まで効かないので、保存した値と並べて見せる。
    pub running_server_settings: crate::api::server_settings::ServerSettings,
    /// 詳しいログの切り替えと、ログファイルの置き場 (→ `api::logs`)。
    pub log: crate::logging::LogControl,
    /// `config.toml` の読み・書き換えを直列化する。2人が同時に保存すると、片方の書き換えが消えるため。
    pub config_write_lock: Arc<Mutex<()>>,
    /// アップロード1件あたりの最大バイト数(`config.upload.max_size_mb`から算出)。
    /// multipartをストリーミングで受け取る際、この値を超えたら
    /// `AppError::FileTooLarge` にする(サーバー全体の受付上限は`contents::router`側の
    /// `DefaultBodyLimit`が別途担う。両者の役割分担はそちらのコメント参照)。
    pub max_upload_bytes: u64,
    /// `contents`テーブルへの書き込みを直列化するための鍵。直列化が要る書き込みは次の2つ。
    /// - blobの「実体確定(commit_blob)→DB書き込み→参照無しGC」。これが無いと、
    ///   あるリクエストがGCの参照カウント確認(COUNT)をした直後に、別リクエストが
    ///   同じハッシュのblobをcommit_blob済みでまだDB行を書いていないタイミングが挟まると、
    ///   GC側が「参照0」と誤判定してその実体を削除してしまい、後から書かれるDB行が
    ///   実体の無いものになる(TOCTOU)。
    /// - group階層の親付け替え(`validate_parent`の循環参照チェック→UPDATE)。
    ///
    /// `validate_parent`は「親の祖先を辿ってselfが含まれないか」を検証するだけの
    /// SELECTなので、検証とUPDATEの間に別リクエストの書き込みが挟まると、
    /// 双方の検証が通った直後に両方がコミットされてA→B→Aのような循環が
    /// 出来てしまう(SQLiteのDEFERREDトランザクションはSELECT時点では
    /// 書き込みロックを取らないため、アプリ側で直列化する必要がある)。
    /// admin操作の頻度を考えると、目的が違う書き込み同士が互いを待たされても
    /// 無視できるオーバーヘッドであり、鍵を2つに分けてロック順序を管理する複雑さを
    /// 避けるために1本化してある。ストリーミング受信自体はこの鍵の外で行うため、
    /// 大きいファイルの受信中に他の書き込みをブロックすることはない。単一プロセス内
    /// のみで意味を持つ(`SqlitePool`同様、複数プロセスでの共有運用は想定していない)。
    /// 作成もこの鍵で削除と直列化する。親の検証の後、書き込む前に親が消えると外部キー制約
    /// 違反 (500) になるため。軸の追加・更新・削除・値の辞書の一括更新も、同じ理由で
    /// アーカイブの存在確認から書き込みまでをこの鍵で直列化する。
    /// アイテムの公開切り替えと値の辞書の一括更新は、公開範囲の判定 (他人の`private`なら
    /// 404) から書き込みまでをこの鍵の内側で行う。判定の後に公開範囲が変わるのを防ぐため。
    pub contents_write_lock: Arc<Mutex<()>>,
    /// アーカイブごとの走査の排他 (→ docs/archive.md「スキャン」)。
    pub archive_scans: Arc<ArchiveScans>,
    /// admin数の不変条件に関わる書き込み(権限変更・削除)を直列化するための鍵
    /// (→ docs/access.md「admin の最後の1人」)。`users`テーブルへの書き込み全般ではなく、この
    /// 不変条件を検査する2箇所(update_role/delete_user)と、下に書く箇所が対象。
    /// 権限変更・削除は「adminの人数を数える→更新する」という2手順で、SQLiteの
    /// DEFERREDトランザクションはSELECT時点では書き込みロックを取らない。この鍵が
    /// 無いと、2人いるadminのうち別々のリクエストがそれぞれ相手を最後の1人と
    /// 誤認したまま両方の降格・削除を許してしまう(admin不在の状態が生まれる)、
    /// あるいは片方がSQLite側の書き込みロック競合で500になる。`contents`とは
    /// 無関係なテーブルのため`contents_write_lock`とは分ける。
    /// ただしユーザーの削除は作成者の記録を消すため`contents_write_lock`も取る
    /// (→ docs/access.md「ユーザーの削除と作成者」)。両方を取るのはそこだけで、順はこの鍵が先。
    /// 初回セットアップの管理者作成も、「adminが0人」の確認から作成までをこの鍵で
    /// 直列化する(→ `api::setup`)。
    /// ユーザーの作成とロールの変更は、Free の上限を数えてから書くまでをこの鍵で直列化する
    /// (→ `api::free_limit`)。
    pub users_write_lock: Arc<Mutex<()>>,
    /// 初回セットアップ用に発行したトークン(→ docs/access.md「初回セットアップ」)。
    pub setup_tokens: Arc<SetupTokens>,
    /// バックアップを作る・戻すときの作業用の置き場 (`AppDirs::backup_work_dir`)。
    pub backup_work_dir: PathBuf,
    /// 受け取って、戻すのを待っているバックアップ (→ `api::backup`)。
    pub restore_staging: Arc<crate::api::backup::RestoreStaging>,
    /// リストアで消したセッション。書き戻させないために覚えておく (→ `session::Store`)。
    pub revoked_sessions: Arc<crate::session::RevokedSessions>,
    /// サーバーの PC にフォルダー選択の窓を出す手段 (→ `api::roots`、docs/folders.md「選び方」)。
    pub folder_picker: crate::folder_picker::FolderPicker,
    /// 他の端末に mDNS の名前 (→ `crate::mdns`) とあわせて案内するポート。
    /// 全インターフェースで待ち受けていないときは`None`。
    pub lan_port: Option<u16>,
    /// Free か Pro か。Free なら個数の上限で止める (→ `api::free_limit`)。
    pub pro: Arc<crate::pro::Pro>,
    /// 窓口とのやり取りの状態 (→ `api::pro`)。裏の確かめが重ならないように持つ。
    pub pro_renewal: Arc<crate::api::pro::Renewal>,
}

/// 走査中のアーカイブidの集合。
///
/// `contents_write_lock` と分けているのは、走査が数十秒に及びうるためで、
/// その間コンテンツ全体の書き込みを止めるわけにいかない。粒度もアーカイブ単位で
/// よく、別のアーカイブの走査は並行してよい。
///
/// ADR: 待たせずに 409 を返す。走査は二重クリックで要求されるのが主なので、
/// 待たせて同じ走査をもう一度実行するより、実行中であることを伝える方がよい。
///
/// `tokio::sync::Mutex` ではなく `std::sync::Mutex` を使う。保持するのは集合を
/// 触る一瞬だけで、この鍵を持ったまま await しないため。
#[derive(Default)]
pub struct ArchiveScans {
    running: StdMutex<HashSet<i64>>,
}

impl ArchiveScans {
    /// 走っている走査が無いか。
    pub fn is_idle(&self) -> bool {
        self.running
            .lock()
            .expect("archive_scans lock poisoned")
            .is_empty()
    }

    /// `id` の走査を開始する。既に走査中なら `Conflict`。
    /// 返り値を保持している間だけ排他が有効で、drop すると解放される。
    pub fn acquire(self: &Arc<Self>, id: i64) -> Result<ArchiveScanGuard, crate::error::AppError> {
        let mut running = self.running.lock().expect("archive_scans lock poisoned");
        if !running.insert(id) {
            return Err(crate::error::AppError::Conflict(
                "this archive is already being scanned".to_string(),
            ));
        }
        Ok(ArchiveScanGuard {
            scans: Arc::clone(self),
            id,
        })
    }
}

/// `ArchiveScans::acquire` の返り値。drop で解放する。
///
/// 早期 return の多いハンドラで解放を書き漏らさないよう、明示的な解放関数は持たない。
pub struct ArchiveScanGuard {
    scans: Arc<ArchiveScans>,
    id: i64,
}

impl Drop for ArchiveScanGuard {
    fn drop(&mut self) {
        // 鍵が毒されていても解放は諦めてよい。毒されるのは他スレッドが集合を
        // 触っている最中に panic した場合で、そのプロセスは既に壊れている。
        if let Ok(mut running) = self.scans.running.lock() {
            running.remove(&self.id);
        }
    }
}
