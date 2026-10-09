//! `build_app` 経由でルーティング・セッションCookieの発行/検証まで含めて確認する
//! HTTPレベルの統合テスト。
//! 機能ごとのファイルに分け、2つ以上のファイルで使う補助だけをここに置く。

use std::net::SocketAddr;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use sqlx::SqlitePool;
use tower::ServiceExt;
use tower_sessions::cookie::Key;
use weblav::config::{AppDirs, Config, LogOutput};
use weblav::folder_picker::FolderPicker;
use weblav::{auth, build_app};

mod anonymous;
mod archive;
mod axes;
mod backup;
mod contents;
mod guards;
mod images;
mod inheritance;
mod items;
mod link_previews;
mod links_file;
mod login;
mod minimal_input;
mod private;
mod pro;
mod remote_files;
mod roots;
mod search;
mod settings;
mod setup;
mod system;
mod upload;
mod users;

// 使うのは `TempDir`・`project_temp_dir` だけで、残りは未使用になるため dead_code を許可する。
#[allow(dead_code)]
#[path = "../../src/test_support.rs"]
mod test_support;

async fn insert_user(pool: &SqlitePool, username: &str, password: &str) {
    let hash = auth::hash_password(password).expect("failed to hash password");
    sqlx::query("INSERT INTO users (username, password_hash) VALUES (?, ?)")
        .bind(username)
        .bind(hash)
        .execute(pool)
        .await
        .expect("failed to insert test user");
}

async fn insert_admin(pool: &SqlitePool, username: &str, password: &str) {
    let hash = auth::hash_password(password).expect("failed to hash password");
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')")
        .bind(username)
        .bind(hash)
        .execute(pool)
        .await
        .expect("failed to insert test admin");
}

/// グループを直接INSERTし、idを返す。
async fn insert_group_under(
    pool: &SqlitePool,
    title: &str,
    visibility: &str,
    parent_id: Option<i64>,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO contents (type, parent_id, title, visibility) \
         VALUES ('group', ?, ?, ?) RETURNING id",
    )
    .bind(parent_id)
    .bind(title)
    .bind(visibility)
    .fetch_one(pool)
    .await
    .expect("グループを挿入できなかった")
}

/// 公開範囲と作成者を指定して link を直接INSERTし、idを返す。
async fn insert_link_by(
    pool: &SqlitePool,
    title: &str,
    visibility: &str,
    created_by: Option<i64>,
    parent_id: Option<i64>,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO contents (type, parent_id, title, url, visibility, created_by) \
         VALUES ('link', ?, ?, 'https://example.com', ?, ?) RETURNING id",
    )
    .bind(parent_id)
    .bind(title)
    .bind(visibility)
    .bind(created_by)
    .fetch_one(pool)
    .await
    .expect("linkを挿入できなかった")
}

/// folder を直接INSERTし、idを返す。
async fn insert_folder_by(
    pool: &SqlitePool,
    title: &str,
    path: impl AsRef<std::path::Path>,
    visibility: &str,
    created_by: Option<i64>,
    parent_id: Option<i64>,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO contents (type, parent_id, title, path, visibility, created_by) \
         VALUES ('folder', ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(parent_id)
    .bind(title)
    .bind(path.as_ref().display().to_string())
    .bind(visibility)
    .bind(created_by)
    .fetch_one(pool)
    .await
    .expect("folderを挿入できなかった")
}

/// archive を直接INSERTし、idを返す。
async fn insert_archive_by(
    pool: &SqlitePool,
    title: &str,
    path: impl AsRef<std::path::Path>,
    visibility: &str,
    created_by: Option<i64>,
    parent_id: Option<i64>,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO contents (type, parent_id, title, path, visibility, created_by) \
         VALUES ('archive', ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(parent_id)
    .bind(title)
    .bind(path.as_ref().display().to_string())
    .bind(visibility)
    .bind(created_by)
    .fetch_one(pool)
    .await
    .expect("archiveを挿入できなかった")
}

/// `test_app` の呼び出しごとに増える連番。テスト間で blobs_dir が衝突しないよう、
/// data_dir の名前に使う (スレッドIDは再利用され得るので使わない)。
static TEST_APP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// この実行の `test_app_with_blobs` の `data_dir` をまとめて置く場所。
///
/// 自動では片付けない。1回の実行で数十KBしか増えず、`cargo clean` で `target/` ごと消えるため。
/// 名前はプロセスIDと開始時刻。プロセスIDだけだと、再利用されたときに前回の分とぶつかる。
///
/// OS の一時ディレクトリに置かないのは、普段見ない場所に溜まり続けて気づけないため。
fn test_app_run_dir() -> &'static std::path::Path {
    static RUN_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    RUN_DIR.get_or_init(|| {
        let root =
            std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("weblav-api-app-data");
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("時計が1970年より前を指している")
            .as_nanos();
        root.join(format!("{}-{started}", std::process::id()))
    })
}

/// テスト用アプリの `data_dir` を、呼ぶたびに別の名前で返す (作りはしない)。
fn next_app_dir() -> std::path::PathBuf {
    let seq = TEST_APP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    test_app_run_dir().join(seq.to_string())
}

/// 設定とデータの置き場を `dir` 1つにまとめたアプリを組み立てる。
/// 置き場の中身 (blob・config.toml など) を直接確かめるテストで使う。
async fn app_in(pool: SqlitePool, config: &Config, dir: &std::path::Path) -> Router {
    app_with_picker(pool, config, dir, FolderPicker::new(|| Ok(None))).await
}

/// `app_in` の、フォルダー選択の窓 (→ `weblav::folder_picker`) を差し替えられる版。
async fn app_with_picker(
    pool: SqlitePool,
    config: &Config,
    dir: &std::path::Path,
    folder_picker: FolderPicker,
) -> Router {
    build_app(
        pool,
        Key::generate(),
        config,
        &AppDirs {
            config_dir: dir.to_path_buf(),
            data_dir: dir.to_path_buf(),
        },
        None,
        folder_picker,
    )
    .await
    .expect("failed to build app")
}

/// アップロード系テストはblobの実体(ファイルの有無)を直接検証したいので、
/// `blobs_dir`のパスも一緒に返すバリアント。
///
/// `data_dir`は`blobs_dir`の親。folderの登録先に使う`temp_test_dir`とは別の
/// ディレクトリの下に置くので、その祖先にはならない
/// (データ置き場を巻き込むパスは配れない。→ `api::roots::is_within_roots`)。
async fn test_app_with_blobs(pool: SqlitePool) -> (Router, std::path::PathBuf) {
    let data_dir = next_app_dir();
    register_content_root(&pool).await;
    let app = app_in(pool, &Config::default(), &data_dir).await;
    let dirs = AppDirs {
        config_dir: data_dir.clone(),
        data_dir,
    };
    (app, dirs.blobs_dir())
}

async fn test_app(pool: SqlitePool) -> Router {
    test_app_with_blobs(pool).await.0
}

/// 開発版だけが信じる鍵で署名した Pro の証明 (→ `weblav::pro`)。
const DEV_PRO: &str = include_str!("../fixtures/dev-pro.json");

/// Pro で動くアプリ。Free の上限 (→ docs/pro.md「上限を数えて止める」) を超える数を API で作るテスト用。
async fn test_app_pro(pool: SqlitePool) -> Router {
    test_app_with_pro_file(pool, DEV_PRO).await
}

/// `pro.json` に `contents` を置いてから組み立てる。
async fn test_app_with_pro_file(pool: SqlitePool, contents: &str) -> Router {
    let data_dir = next_app_dir();
    std::fs::create_dir_all(&data_dir).expect("データ置き場を作れなかった");
    let dirs = AppDirs {
        config_dir: data_dir.clone(),
        data_dir: data_dir.clone(),
    };
    std::fs::write(dirs.pro_path(), contents).expect("pro.json を書けなかった");
    register_content_root(&pool).await;
    app_in(pool, &Config::default(), &data_dir).await
}

/// `blobs_dir`直下(`tmp/`を除く)にあるblobファイルの数を数える。コミット済みの
/// blob(重複排除の単位)がいくつ残っているかをテストから確認するためのヘルパー。
fn count_committed_blobs(blobs_dir: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(blobs_dir) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name() != "tmp")
        .count()
}

/// multipart/form-dataのリクエストを組み立てる(テスト用の最小実装。外部クレートを
/// 増やさないため手書きする)。`file_field`は`(フィールド名, ファイル名, 内容)`。
fn multipart_request(
    method: &str,
    uri: &str,
    cookie: &str,
    text_fields: &[(&str, &str)],
    file_field: Option<(&str, &str, &[u8])>,
) -> Request<Body> {
    const BOUNDARY: &str = "----weblavtestboundary0123456789";
    let mut body = Vec::new();
    for (name, value) in text_fields {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    if let Some((name, file_name, content)) = file_field {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .expect("リクエストを組み立てられなかった")
}

/// CookieとJSON本文を必要なときだけ付けて要求を組み立てる。
fn build_request(
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    json: Option<&str>,
) -> Request<Body> {
    let request = Request::builder().method(method).uri(uri);
    let request = match cookie {
        Some(cookie) => request.header(header::COOKIE, cookie),
        None => request,
    };
    match json {
        Some(json) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string()))
            .expect("リクエストを組み立てられなかった"),
        None => request
            .body(Body::empty())
            .expect("リクエストを組み立てられなかった"),
    }
}

/// 応答をそのまま返す。ヘッダーを見るテストで使う。
async fn send_raw(app: Router, request: Request<Body>) -> Response {
    app.oneshot(request)
        .await
        .expect("リクエストの実行に失敗した")
}

/// JSON要求を送り、`(ステータス, レスポンスボディ)`を返す。
async fn send_json(
    app: Router,
    method: &str,
    uri: &str,
    cookie: &str,
    json: &str,
) -> (StatusCode, String) {
    send(app, build_request(method, uri, Some(cookie), Some(json))).await
}

/// Cookieを付けずにJSON要求を送り、`(ステータス, レスポンスボディ)`を返す。
async fn send_anon_json(app: Router, method: &str, uri: &str, json: &str) -> (StatusCode, String) {
    send(app, build_request(method, uri, None, Some(json))).await
}

/// 組み立て済みのリクエストを送り、`(ステータス, レスポンスボディ)` を返す。
async fn send(app: Router, request: Request<Body>) -> (StatusCode, String) {
    let response = send_raw(app, request).await;
    let status = response.status();
    (status, body_string(response).await)
}

async fn body_string(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("レスポンスボディを読み取れなかった");
    String::from_utf8(bytes.to_vec()).expect("レスポンスボディがUTF-8ではなかった")
}

fn extract_id(body: &str) -> i64 {
    serde_json::from_str::<serde_json::Value>(body)
        .expect("レスポンスをJSONとして読めなかった")["id"]
        .as_i64()
        .expect("idが返らなかった")
}

async fn created_id(response: Response) -> i64 {
    extract_id(&body_string(response).await)
}

/// クッキーのみ付けたボディ無しリクエストを送る(GET/DELETE用)。
async fn send_empty(app: Router, method: &str, uri: &str, cookie: &str) -> (StatusCode, String) {
    send(app, build_request(method, uri, Some(cookie), None)).await
}

/// Cookieを付けずにボディ無し要求を送り、`(ステータス, レスポンスボディ)`を返す。
async fn send_anon(app: Router, method: &str, uri: &str) -> (StatusCode, String) {
    send(app, build_request(method, uri, None, None)).await
}

/// ログインしてセッションCookieを返す。`contents` 系テストで
/// 「まずログインする」を毎回書かずに済ませるためのヘルパー。
async fn login(app: Router, username: &str, password: &str) -> String {
    let response = send_raw(
        app,
        build_request(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(&format!(
                r#"{{"username":"{username}","password":"{password}"}}"#
            )),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    set_cookie(&response)
}

/// 管理者を1人作ってログインし、セッションCookieを返す。
/// 管理者の名前を検証に使わないテスト向け。
async fn admin_cookie(app: Router, pool: &SqlitePool) -> String {
    insert_admin(pool, "admin-alice", "correct-password").await;
    login(app, "admin-alice", "correct-password").await
}

/// 管理者でログイン済みのアプリとセッションCookieを用意する。
async fn admin_app(pool: &SqlitePool) -> (Router, String) {
    let app = test_app(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), pool).await;
    (app, cookie)
}

/// 管理者でログインし、アーカイブを作成・再走査してidを返す。
async fn setup_archive(
    pool: &SqlitePool,
    dir: &std::path::Path,
    extensions: Option<&str>,
) -> (Router, String, i64) {
    let (app, cookie) = admin_app(pool).await;
    let id = create_archive(app.clone(), &cookie, dir, extensions).await;
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (app, cookie, id)
}

/// folderコンテンツのテスト用の一時ディレクトリを作る。Drop で消えるので、テストが途中で
/// 落ちても残らない。存在しないパスやファイルのパスが要るなら `temp_test_path` を使う。
///
/// この実行の置き場所 (`test_app_run_dir`) の下に置く。data_dir (`<連番>`) とは
/// 別のディレクトリの下なので、その祖先にはならない。
///
/// この親そのものが「公開できるフォルダー」として登録される (→ `register_content_root`)。
fn temp_test_dir(name: &str) -> test_support::TempDir {
    test_support::TempDir::at(temp_test_path(name))
}

/// `temp_test_dir` の場所。作らずにパスだけ返す (存在しないパスやファイルを渡すテスト用)。
fn temp_test_path(name: &str) -> std::path::PathBuf {
    content_root().join(format!("{name}-{:?}", std::thread::current().id()))
}

/// `temp_test_dir` の親。テストの土台として、ここを「公開できるフォルダー」に登録する。
fn content_root() -> std::path::PathBuf {
    let parent = test_app_run_dir().join("folders");
    std::fs::create_dir_all(&parent).expect("一時ディレクトリの親を作れなかった");
    parent
}

/// パスを使う操作が通るように、`temp_test_dir` の親を「公開できるフォルダー」へ入れておく
/// (→ docs/folders.md「公開できるフォルダー」)。登録が1件も無いと、`folder`/`archive` は何も登録できない。
///
/// 「公開できるフォルダー」そのものの登録・削除を試すテストは、この外 (`temp_root_dir`) を
/// 使う。件数や起点が、ここの登録に左右されないようにするため。
async fn register_content_root(pool: &SqlitePool) {
    let canonical = std::fs::canonicalize(content_root()).expect("canonicalize できなかった");
    register_root(pool, &canonical).await;
}

/// 「公開できるフォルダー」へ直接入れる。登録の口はループバック限定なので、土台を
/// 用意するだけのテストは API を通さない。**canonicalize 済みのパスを渡すこと**。
///
/// 名前はパスと同じにする。名前は重なりを拒むので、パスごとに違う値にしておく。
async fn register_root(pool: &SqlitePool, canonical: &std::path::Path) {
    let path = canonical_path_as_api_returns_it(canonical);
    sqlx::query("INSERT OR IGNORE INTO roots (name, path) VALUES (?, ?)")
        .bind(&path)
        .bind(&path)
        .execute(pool)
        .await
        .expect("公開できるフォルダーを登録できなかった");
}

/// 「公開できるフォルダー」の登録を試すテスト用の一時ディレクトリ (Drop で消える)。`temp_test_dir` とは別の親に置く。
fn temp_root_dir(name: &str) -> test_support::TempDir {
    test_support::TempDir::at(temp_root_path(name))
}

/// `temp_root_dir` の場所。作らずにパスだけ返す。
fn temp_root_path(name: &str) -> std::path::PathBuf {
    let parent = test_app_run_dir().join("roots");
    std::fs::create_dir_all(&parent).expect("一時ディレクトリの親を作れなかった");
    parent.join(format!("{name}-{:?}", std::thread::current().id()))
}

/// `canonicalize()` の結果を、APIが返す形の文字列にする。
///
/// Windows の `canonicalize()` は verbatim 形式 (`\\?\C:\...`) を返すが、APIは保存時に
/// その接頭辞を落とす (→ `api::fs::simplify_verbatim`)。本体の関数は非公開モジュールに
/// あって統合テストからは呼べないため、ここではテストが渡すドライブパスに限って同じ
/// 処理をする (UNC と 260 文字超はテストに出てこない)。
///
/// **接頭辞を落としたうえで文字列として比較する**。両辺を `canonicalize()` に通す形だと、
/// APIが呼び出し側の生の入力をそのまま返しても通ってしまい、
/// 「正規化して保存している」ことの検証にならない。
fn canonical_path_as_api_returns_it(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

fn set_cookie(response: &Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("login should set a cookie")
        .split(';')
        .next()
        .expect("splitは少なくとも1要素を返す")
        .to_string()
}

const RECOVER_URI: &str = "/api/v1/auth/recover";

fn json_str(body: &str, key: &str) -> String {
    parse_json(body)[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} が返らなかった: {body}"))
        .to_string()
}

async fn recover(app: Router, username: &str, code: &str, new_password: &str) -> Response {
    send_raw(
        app,
        build_request(
            "POST",
            RECOVER_URI,
            None,
            Some(&format!(
                r#"{{"username":"{username}","recoveryCode":"{code}","newPassword":"{new_password}"}}"#
            )),
        ),
    )
    .await
}

/// DB上のユーザーidを引く。作成者を直接INSERTで入れるテストで使う。
async fn db_user_id(pool: &SqlitePool, username: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(pool)
        .await
        .expect("ユーザーのidを取得できなかった")
}

/// アーカイブを1件作り、そのidを返す。`dir` は登録先の絶対パス。
async fn create_archive(
    app: Router,
    cookie: &str,
    dir: &std::path::Path,
    extensions: Option<&str>,
) -> i64 {
    let path_json = json_string(&dir.display().to_string());
    let extensions_json = match extensions {
        Some(ext) => format!(
            r#","extensions":{}"#,
            serde_json::to_string(ext).expect("拡張子をJSON文字列にできなかった")
        ),
        None => String::new(),
    };
    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        cookie,
        &format!(
            r#"{{"type":"archive","title":"英検 過去問","path":{path_json}{extensions_json}}}"#
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    extract_id(&body)
}

async fn rescan(app: Router, cookie: &str, id: i64) -> (StatusCode, String) {
    send_empty(
        app,
        "POST",
        &format!("/api/v1/contents/{id}/rescan"),
        cookie,
    )
    .await
}

fn parse_json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("レスポンスをJSONとして読めなかった")
}

/// 軸を1本作り、その id を返す。
async fn create_axis(app: Router, cookie: &str, archive_id: i64, json: &str) -> i64 {
    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/v1/contents/{archive_id}/axes"),
        cookie,
        json,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    extract_id(&body)
}

/// 軸の値の辞書を一括更新する。
async fn put_axis_values(app: Router, cookie: &str, archive_id: i64, axis_id: i64, json: &str) {
    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{archive_id}/axes/{axis_id}/values"),
        cookie,
        json,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// 閲覧用のアーカイブ一覧を取得する。`query` は軸名を含みうるため、キー・値とも
/// パーセントエンコードする (`http::Uri` は非ASCIIを受け付けないため)。
async fn view_archive(
    app: Router,
    cookie: &str,
    archive_id: i64,
    query: &[(&str, &str)],
) -> (StatusCode, String) {
    let query_string: String = query
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                utf8_percent_encode(key, NON_ALPHANUMERIC),
                utf8_percent_encode(value, NON_ALPHANUMERIC)
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    let uri = if query_string.is_empty() {
        format!("/api/v1/contents/{archive_id}/archive")
    } else {
        format!("/api/v1/contents/{archive_id}/archive?{query_string}")
    };
    send(
        app,
        build_request("GET", &uri, (!cookie.is_empty()).then_some(cookie), None),
    )
    .await
}

/// アーカイブ配下のアイテムを全件公開にする (テストの前準備用)。
async fn publish_all_items(pool: &SqlitePool, archive_id: i64) {
    sqlx::query!(
        "UPDATE archive_items SET published = 1 WHERE archive_id = ?",
        archive_id
    )
    .execute(pool)
    .await
    .expect("公開フラグを立てられなかった");
}

/// 一覧APIから`username`のidを引く。ユーザー管理APIはid採番をレスポンスで返すが、
/// テスト側は作成時のusernameしか持たないため、以降の操作(role変更等)はこれで引いたidを使う。
async fn user_id(app: Router, cookie: &str, username: &str) -> i64 {
    let (status, body) = send_empty(app, "GET", "/api/v1/admin/users", cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    json.as_array()
        .expect("配列でない")
        .iter()
        .find(|u| u["username"] == username)
        .unwrap_or_else(|| panic!("{username}が見つからない: {json}"))["id"]
        .as_i64()
        .expect("idが数値でない")
}

const SETUP_STATUS_URI: &str = "/api/v1/setup/status";
const SETUP_TOKEN_URI: &str = "/api/v1/setup/token";

/// 接続元を載せる。実サーバーは `into_make_service_with_connect_info` で載せるが、
/// `oneshot` では付かないため、テスト側で入れる (→ `api::setup`)。
fn from_peer(mut request: Request<Body>, addr: &str) -> Request<Body> {
    request.extensions_mut().insert(ConnectInfo(
        addr.parse::<SocketAddr>()
            .expect("アドレスを解釈できなかった"),
    ));
    request
}

/// トレイ・ブラウザーが開くのと同じ、ループバック宛の要求にする。
fn from_loopback(request: Request<Body>) -> Request<Body> {
    with_host(from_peer(request, "127.0.0.1:50000"), "127.0.0.1:3000")
}

fn with_host(mut request: Request<Body>, host: &str) -> Request<Body> {
    request.headers_mut().insert(
        header::HOST,
        host.parse().expect("Host を組み立てられなかった"),
    );
    request
}

/// トレイが押されたときと同じ手順でトークンを受け取る。
async fn issue_setup_token(app: Router) -> String {
    let (status, token) = send(
        app,
        from_loopback(build_request("POST", SETUP_TOKEN_URI, None, None)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    token
}

const ROOTS_URI: &str = "/api/v1/admin/roots";

/// サーバーの PC の管理者として、登録・一覧・削除の要求を作る。
fn roots_request(method: &str, uri: &str, cookie: &str, body: Option<&str>) -> Request<Body> {
    from_loopback(build_request(method, uri, Some(cookie), body))
}

/// 登録できる場所を1つ作る。`data_dir` の祖先にならない場所に置く。
fn root_dir(name: &str) -> test_support::TempDir {
    let path = temp_root_path(name);
    std::fs::create_dir_all(&path).expect("フォルダーを作れなかった");
    test_support::TempDir::at(std::fs::canonicalize(&path).expect("canonicalize できなかった"))
}

fn json_path(path: &str) -> String {
    format!(r#"{{"path":{}}}"#, json_string(path))
}

/// JSON の文字列リテラル (引用符付き) にする。Windows の `\` も逃がす。
fn json_string(path: &str) -> String {
    serde_json::to_string(path).expect("パスをJSON文字列にできなかった")
}

/// エラーの応答の状態と `code` を確かめる。
#[track_caller]
fn assert_error(status: StatusCode, body: &str, expected_status: StatusCode, expected_code: &str) {
    assert_error_case(status, body, expected_status, expected_code, "");
}

/// `assert_error` の、表で回すときの版。`label` でどの場合に落ちたかを出す。
#[track_caller]
fn assert_error_case(
    status: StatusCode,
    body: &str,
    expected_status: StatusCode,
    expected_code: &str,
    label: &str,
) {
    assert_eq!(status, expected_status, "{label}: {body}");
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| json["error"]["code"].as_str().map(str::to_string));
    assert_eq!(code.as_deref(), Some(expected_code), "{label}: {body}");
}

/// 期限の切れていないリンクのカードの情報を入れる。取り直しで外へつながないようにするため。
async fn insert_fresh_preview(pool: &SqlitePool, url: &str, image_file: Option<&str>) {
    sqlx::query(
        "INSERT INTO link_previews (url, title, site_name, image_file, fresh_until, checked_at) \
         VALUES (?, 'Example', 'Example Site', ?, 9999999999, 9999999999)",
    )
    .bind(url)
    .bind(image_file)
    .execute(pool)
    .await
    .expect("カードの情報を入れられるはず");
}
