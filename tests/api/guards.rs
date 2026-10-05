//! 全部の API にかかる守り (ログイン・管理者の権限・この PC の中からだけの口)。API の一覧を表にして回す。

use super::*;

/// ログインが要る API の一覧。`{id}` には存在しない ID を入れる (守りが先に効けば 404 にならない)。
/// 匿名でも公開範囲によって開ける閲覧の口 (`Viewer` を取るもの) は入れない。
const LOGIN_REQUIRED_APIS: &[(&str, &str)] = &[
    ("GET", "/api/v1/auth/me"),
    ("PUT", "/api/v1/auth/password"),
    ("PUT", "/api/v1/auth/username"),
    ("POST", "/api/v1/auth/recovery-code"),
    ("GET", "/api/v1/admin/contents"),
    ("POST", "/api/v1/contents"),
    ("POST", "/api/v1/contents/link-metadata"),
    ("POST", "/api/v1/contents/upload"),
    ("PUT", "/api/v1/contents/9999"),
    ("DELETE", "/api/v1/contents/9999"),
    ("PUT", "/api/v1/contents/9999/upload"),
    ("PUT", "/api/v1/contents/9999/creator"),
    ("POST", "/api/v1/contents/9999/rescan"),
    ("GET", "/api/v1/contents/9999/axes"),
    ("POST", "/api/v1/contents/9999/axes"),
    ("PUT", "/api/v1/contents/9999/axes"),
    ("POST", "/api/v1/contents/9999/axes/import/preview"),
    ("PUT", "/api/v1/contents/9999/axes/1"),
    ("DELETE", "/api/v1/contents/9999/axes/1"),
    ("GET", "/api/v1/contents/9999/axes/1/values"),
    ("PUT", "/api/v1/contents/9999/axes/1/values"),
    ("POST", "/api/v1/contents/9999/axes/1/values/preview"),
    ("GET", "/api/v1/contents/9999/dir-levels"),
    ("GET", "/api/v1/contents/9999/filename-words"),
    ("GET", "/api/v1/contents/9999/items"),
    ("PUT", "/api/v1/contents/9999/items"),
    ("PUT", "/api/v1/contents/9999/items/1"),
    ("GET", "/api/v1/contents/9999/items/1/manage-download"),
    ("GET", "/api/v1/admin/fs/dirs"),
    ("GET", "/api/v1/admin/fs/count"),
    ("GET", "/api/v1/admin/backup"),
    ("POST", "/api/v1/admin/backup/restore/stage"),
    ("POST", "/api/v1/admin/backup/restore"),
    ("GET", "/api/v1/admin/log-settings"),
    ("PUT", "/api/v1/admin/log-settings"),
    ("GET", "/api/v1/admin/logs"),
    ("GET", "/api/v1/admin/pro"),
    ("DELETE", "/api/v1/admin/pro"),
    ("POST", "/api/v1/admin/pro/check"),
    ("POST", "/api/v1/admin/pro/link"),
    ("DELETE", "/api/v1/admin/pro/link"),
    ("POST", "/api/v1/admin/pro/link/code"),
    ("POST", "/api/v1/admin/pro/link/poll"),
    ("GET", "/api/v1/admin/server-settings"),
    ("PUT", "/api/v1/admin/server-settings"),
    ("PUT", "/api/v1/admin/site-settings"),
    ("GET", "/api/v1/admin/users"),
    ("POST", "/api/v1/admin/users"),
    ("DELETE", "/api/v1/admin/users/9999"),
    ("PUT", "/api/v1/admin/users/9999/password"),
    ("PUT", "/api/v1/admin/users/9999/role"),
    ("PUT", "/api/v1/admin/users/9999/username"),
];

/// 管理者だけの API のうち、この PC の中からだけの口 (`LOCAL_ONLY_APIS`) を除いたもの。
/// `{id}` は表を回すテストが実在する ID に差し替える。
const ADMIN_ONLY_APIS: &[(&str, &str)] = &[
    ("PUT", "/api/v1/contents/{content}/creator"),
    ("GET", "/api/v1/admin/backup"),
    ("POST", "/api/v1/admin/backup/restore/stage"),
    ("POST", "/api/v1/admin/backup/restore"),
    ("GET", "/api/v1/admin/log-settings"),
    ("PUT", "/api/v1/admin/log-settings"),
    ("GET", "/api/v1/admin/logs"),
    ("GET", "/api/v1/admin/pro"),
    ("DELETE", "/api/v1/admin/pro"),
    ("POST", "/api/v1/admin/pro/check"),
    ("POST", "/api/v1/admin/pro/link"),
    ("DELETE", "/api/v1/admin/pro/link"),
    ("POST", "/api/v1/admin/pro/link/code"),
    ("POST", "/api/v1/admin/pro/link/poll"),
    ("GET", "/api/v1/admin/server-settings"),
    ("PUT", "/api/v1/admin/server-settings"),
    ("PUT", "/api/v1/admin/site-settings"),
    ("GET", "/api/v1/admin/users"),
    ("POST", "/api/v1/admin/users"),
    ("DELETE", "/api/v1/admin/users/{user}"),
    ("PUT", "/api/v1/admin/users/{user}/password"),
    ("PUT", "/api/v1/admin/users/{user}/role"),
    ("PUT", "/api/v1/admin/users/{user}/username"),
];

/// この PC の中からだけ使える口 (→ `api::local::LocalRequest`)。「公開できるフォルダ」は管理者だけの口でもある。
const LOCAL_ONLY_APIS: &[(&str, &str)] = &[
    ("GET", "/api/v1/setup/status"),
    ("POST", "/api/v1/setup/token"),
    ("POST", "/api/v1/setup/token/verify"),
    ("POST", "/api/v1/setup/admin"),
    ("POST", "/api/v1/setup/restore/stage"),
    ("POST", "/api/v1/setup/restore"),
    ("GET", "/api/v1/admin/roots"),
    ("POST", "/api/v1/admin/roots"),
    ("POST", "/api/v1/admin/roots/pick"),
    ("PUT", "/api/v1/admin/roots/9999"),
    ("DELETE", "/api/v1/admin/roots/9999"),
];

fn is_roots_api(uri: &str) -> bool {
    uri.starts_with(ROOTS_URI)
}

/// 本文を付けずに送る。本文の検証より先にログインを確かめていなければ、401 でなく 415・422 などになる。
/// 「公開できるフォルダ」は LAN からだと 404 になるので、この PC の中から送る。
#[sqlx::test]
async fn login_required_apis_reject_requests_without_a_session(pool: SqlitePool) {
    let app = test_app(pool).await;

    let roots = LOCAL_ONLY_APIS
        .iter()
        .filter(|(_, uri)| is_roots_api(uri))
        .map(|(method, uri)| (*method, *uri, true));
    let cases = LOGIN_REQUIRED_APIS
        .iter()
        .map(|(method, uri)| (*method, *uri, false))
        .chain(roots);
    for (method, uri, local) in cases {
        let request = build_request(method, uri, None, None);
        let request = if local {
            from_loopback(request)
        } else {
            request
        };
        let (status, body) = send(app.clone(), request).await;
        assert_error_case(
            status,
            &body,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            &format!("{method} {uri}"),
        );
    }
}

/// 一般ユーザーには 403 を返す。未ログインの 401 と区別できるコードにする
/// (frontend は 401 をセッション切れとしてログイン画面へ送るため)。拒んだ操作は何も変えない。
#[sqlx::test]
async fn admin_apis_reject_regular_users(pool: SqlitePool) {
    insert_user(&pool, "taro", "password").await;
    insert_user(&pool, "other", "password").await;
    let user_id = db_user_id(&pool, "other").await;
    let content_id = insert_link_by(&pool, "社内Wiki", "public", None, None).await;
    let app = test_app(pool.clone()).await;
    let cookie = login(app.clone(), "taro", "password").await;

    let roots = LOCAL_ONLY_APIS
        .iter()
        .filter(|(_, uri)| is_roots_api(uri))
        .map(|(method, uri)| (*method, uri.to_string(), true));
    let cases = ADMIN_ONLY_APIS
        .iter()
        .map(|(method, uri)| {
            let uri = uri
                .replace("{user}", &user_id.to_string())
                .replace("{content}", &content_id.to_string());
            (*method, uri, false)
        })
        .chain(roots);
    for (method, uri, local) in cases {
        let request = build_request(method, &uri, Some(&cookie), None);
        let request = if local {
            from_loopback(request)
        } else {
            request
        };
        let (status, body) = send(app.clone(), request).await;
        assert_error_case(
            status,
            &body,
            StatusCode::FORBIDDEN,
            "forbidden",
            &format!("{method} {uri}"),
        );
    }

    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("ユーザー数を数えられなかった");
    assert_eq!(users, 2);
    let role: String = sqlx::query_scalar("SELECT role FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("役割を引けなかった");
    assert_eq!(role, "user");
    login(app, "other", "password").await;
}

/// LAN の端末には、この口があること自体を見せない。管理者でログインしていても同じ。
/// 壊れた JSON を付けて送る。ボディを読んでから弾くと 400・422 になり、この口が JSON を受けることが分かってしまう。
#[sqlx::test]
async fn local_only_apis_are_not_found_from_the_lan(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    for (method, uri) in LOCAL_ONLY_APIS {
        let body = (*method != "GET").then_some("{");
        let request = with_host(
            from_peer(
                build_request(method, uri, Some(&cookie), body),
                "192.168.1.10:50000",
            ),
            "192.168.1.5:3000",
        );
        let (status, body) = send(app.clone(), request).await;
        assert_error_case(
            status,
            &body,
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("{method} {uri}"),
        );
    }
}
