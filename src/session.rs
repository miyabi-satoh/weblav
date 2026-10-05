//! セッションCookieの署名鍵の解決と `SessionManagerLayer` の組み立て。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use tower_sessions::service::SignedCookie;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store;
use tower_sessions::{Expiry, SessionManagerLayer, SessionStore, cookie::Key};
use tower_sessions_sqlx_store::SqliteStore;

use crate::config::{SessionConfig, create_owner_only_dir, replace_owner_only_file};

const KEY_LEN: usize = 64;

pub type SessionLayer = SessionManagerLayer<Store, SignedCookie>;

/// リストアで消したセッションの id (→ docs/access.md「バックアップとリストア」)。プロセスのメモリにだけ置く。
///
/// 起動し直せば、消す前から続いていた要求は残っていないので、覚えておく必要が無い。
/// `std::sync::Mutex` なのは、持ったまま await しないため (`SetupTokens` と同じ)。
#[derive(Debug, Default)]
pub struct RevokedSessions(Mutex<HashSet<String>>);

impl RevokedSessions {
    pub fn extend(&self, ids: impl IntoIterator<Item = String>) {
        self.lock().extend(ids);
    }

    /// 消せなかったものの登録を取り消す。
    pub fn forget(&self, ids: &[String]) {
        let mut revoked = self.lock();
        for id in ids {
            revoked.remove(id);
        }
    }

    fn contains(&self, id: &Id) -> bool {
        self.lock().contains(&id.to_string())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// `SqliteStore` を包み、リストアで消したセッションを書き戻させない。
///
/// ADR: `with_always_save` なので、リストアの前に始まった要求は、応答を返すときにセッションを
/// 保存し直す。`SqliteStore::save` は行が無ければ作るので、消したセッションが生き返り、
/// 戻した DB で同じ利用者 id を持つ別の人として通ってしまう。
/// 保存した後で消した id かを確かめて消し直す。リストアは消すのと同じトランザクションの中で、
/// 消す前に id を登録するので、保存と削除がどの順に来ても残らない。
#[derive(Debug, Clone)]
pub struct Store {
    inner: SqliteStore,
    revoked: Arc<RevokedSessions>,
}

impl Store {
    pub fn new(inner: SqliteStore, revoked: Arc<RevokedSessions>) -> Self {
        Self { inner, revoked }
    }
}

#[async_trait::async_trait]
impl SessionStore for Store {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        self.inner.create(record).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        self.inner.save(record).await?;
        if self.revoked.contains(&record.id) {
            self.inner.delete(&record.id).await?;
        }
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        self.inner.load(session_id).await
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        self.inner.delete(session_id).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to decode session.secret (must be base64)")]
    Decode(#[source] base64::DecodeError),
    #[error("invalid session key length ({0} bytes, expected 64)")]
    InvalidLength(usize),
    #[error("failed to read/write session key file: {path}")]
    KeyFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// セッションの署名鍵を解決する。
///
/// - `config.secret` が空でなければ、base64デコードしてそのまま使う(64バイト必須)。
/// - 空なら `key_path` を読み書きして自動生成・永続化する(初回起動時に無設定でも動くように)。
///   永続化されたランダム64バイトの鍵は運用者が手で用意するsecretより弱くないため、
///   bind先に関わらず自動生成でよい。`secret` は複数インスタンスで鍵を共有したい場合などの
///   オプションとする。
pub fn resolve_key(config: &SessionConfig, key_path: &Path) -> Result<Key, Error> {
    if !config.secret.is_empty() {
        let bytes = STANDARD.decode(&config.secret).map_err(Error::Decode)?;
        if bytes.len() != KEY_LEN {
            return Err(Error::InvalidLength(bytes.len()));
        }
        return Ok(Key::from(&bytes));
    }

    load_or_generate_key(key_path)
}

fn load_or_generate_key(path: &Path) -> Result<Key, Error> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() == KEY_LEN => Ok(Key::from(&bytes)),
        Ok(bytes) => Err(Error::InvalidLength(bytes.len())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => generate_and_persist_key(path),
        Err(source) => Err(Error::KeyFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn generate_and_persist_key(path: &Path) -> Result<Key, Error> {
    let key = Key::generate();

    if let Some(parent) = path.parent() {
        create_owner_only_dir(parent).map_err(|source| Error::KeyFile {
            path: path.to_path_buf(),
            source,
        })?;
    }

    // 中途半端な(短い)ファイルが残ると、次回起動時に `Key::from` が長さ不足でパニックする恐れがあるため、
    // 一時ファイルに書いてから置き換える。
    replace_owner_only_file(&path.with_extension("tmp"), path, key.master(), false).map_err(
        |source| Error::KeyFile {
            path: path.to_path_buf(),
            source,
        },
    )?;

    Ok(key)
}

/// axumに載せる`SessionManagerLayer`を組み立てる。
///
/// `with_always_save` が無いと `/me` のような読み取りだけのアクセスではセッションが
/// 「更新」されず、`OnInactivity` でも実質「ログインからN日」にしかならないため付与する。
pub fn layer(store: Store, key: Key, config: &SessionConfig) -> SessionLayer {
    SessionManagerLayer::new(store)
        .with_secure(config.secure_cookie)
        .with_expiry(Expiry::OnInactivity(time::Duration::days(
            config.expiry_days,
        )))
        .with_always_save(true)
        .with_signed(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    async fn store(pool: sqlx::SqlitePool) -> (Store, Arc<RevokedSessions>) {
        let inner = SqliteStore::new(pool);
        inner.migrate().await.expect("セッションの表を作れなかった");
        let revoked = Arc::new(RevokedSessions::default());
        (Store::new(inner, Arc::clone(&revoked)), revoked)
    }

    fn record() -> Record {
        Record {
            id: Id::default(),
            data: Default::default(),
            expiry_date: time::OffsetDateTime::now_utc() + time::Duration::hours(1),
        }
    }

    /// リストアの前に始まった要求が、消したセッションを応答のときに保存し直しても残らない。
    #[sqlx::test]
    async fn revoked_session_is_not_brought_back_by_save(pool: sqlx::SqlitePool) {
        let (store, revoked) = store(pool).await;
        let mut record = record();
        store.create(&mut record).await.expect("作れなかった");
        revoked.extend([record.id.to_string()]);
        store.delete(&record.id).await.expect("消せなかった");

        store.save(&record).await.expect("保存できなかった");

        assert!(
            store
                .load(&record.id)
                .await
                .expect("読めなかった")
                .is_none()
        );
    }

    #[sqlx::test]
    async fn other_sessions_are_saved_as_usual(pool: sqlx::SqlitePool) {
        let (store, revoked) = store(pool).await;
        let mut record = record();
        store.create(&mut record).await.expect("作れなかった");
        revoked.extend([Id::default().to_string()]);

        store.save(&record).await.expect("保存できなかった");

        assert!(
            store
                .load(&record.id)
                .await
                .expect("読めなかった")
                .is_some()
        );
    }

    fn config_with_secret(secret: &str) -> SessionConfig {
        SessionConfig {
            secret: secret.to_string(),
            ..Default::default()
        }
    }

    fn key_path(tmp: &TempDir) -> PathBuf {
        tmp.path().join("session.key")
    }

    #[test]
    fn generates_and_persists_key_on_first_call() {
        let tmp = TempDir::new("generates_and_persists_key_on_first_call");
        let config = SessionConfig::default();

        let key1 = resolve_key(&config, &key_path(&tmp)).unwrap();
        assert!(key_path(&tmp).exists());
        assert_eq!(std::fs::read(key_path(&tmp)).unwrap().len(), KEY_LEN);

        let key2 = resolve_key(&config, &key_path(&tmp)).unwrap();
        assert_eq!(key1.master(), key2.master(), "2回目は同じ鍵を返すべき");
    }

    #[test]
    #[cfg(unix)]
    fn generated_key_file_has_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new("generated_key_file_has_owner_only_permissions");
        let config = SessionConfig::default();

        resolve_key(&config, &key_path(&tmp)).unwrap();

        let mode = std::fs::metadata(key_path(&tmp))
            .unwrap()
            .permissions()
            .mode();
        // owner権限は `mode(0o600)` 指定時点のumask次第で厳密に0o600と一致しない環境も
        // あり得るため、ここでは group/other への権限が一切無いことだけを検証する
        // (owner-onlyという要件そのものはこれで十分に確認できる)。
        assert_eq!(mode & 0o077, 0, "{:o}", mode);
    }

    #[test]
    fn explicit_secret_must_be_64_bytes() {
        let tmp = TempDir::new("explicit_secret_must_be_64_bytes");
        let short = STANDARD.encode([0u8; 32]);
        let config = config_with_secret(&short);

        let err = resolve_key(&config, &key_path(&tmp)).unwrap_err();
        assert!(matches!(err, Error::InvalidLength(32)));
    }

    #[test]
    fn explicit_secret_decodes_when_valid() {
        let tmp = TempDir::new("explicit_secret_decodes_when_valid");
        let encoded = STANDARD.encode([7u8; KEY_LEN]);
        let config = config_with_secret(&encoded);

        let key = resolve_key(&config, &key_path(&tmp)).unwrap();
        assert_eq!(key.master(), [7u8; KEY_LEN]);
        assert!(!key_path(&tmp).exists(), "明示指定時は鍵ファイルを作らない");
    }

    #[test]
    fn invalid_base64_secret_is_error() {
        let tmp = TempDir::new("invalid_base64_secret_is_error");
        let config = config_with_secret("not base64!");

        let err = resolve_key(&config, &key_path(&tmp)).unwrap_err();
        assert!(matches!(err, Error::Decode(_)));
    }
}
