//! `weblav-cli` (src/bin/weblav-cli.rs) の中身。開発・テスト用で、リリース版には入れない
//! (使い方を知っていれば誰でも管理者を足せるため。→ docs/access.md「初回セットアップ」)。
//!
//! `weblav` 本体と exe を分けているのは、本体が Windows で GUI サブシステムのため
//! (→ docs/distribution.md「コンソールの扱い」)。コマンドプロンプトは GUI サブシステムの子プロセスの終了を
//! 待たず、リダイレクト先のハンドルも渡さないので、パスワード入力も `>` も成り立たない。

use std::fmt::Display;
use std::io::Write;

use crate::config::AppDirs;
use crate::error::error_chain;

/// `--create-user` の書式。全体の `USAGE` と、引数を誤ったときの案内で同じ文言を使う。
macro_rules! create_user_synopsis {
    () => {
        "weblav-cli --create-user [<username>] [--admin]"
    };
}

pub const USAGE: &str = concat!(
    "usage:\n  ",
    create_user_synopsis!(),
    "\n  weblav-cli --openapi\n  weblav-cli -v | --version"
);

const CREATE_USER_USAGE: &str = concat!("usage: ", create_user_synopsis!());

/// メッセージを標準エラーに出して、終了コード1で終える。
fn fail(message: impl Display) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// コマンドライン引数を処理する。引数が無ければ何もせず false を返す。
pub fn handle_args() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(first) = args.get(1).map(String::as_str) else {
        return false;
    };
    match first {
        "-v" | "--version" => {
            println!("v{}", crate::APP_VERSION);
        }
        // 開発者・API連携者向け(このアプリを使うだけの一般利用者は使わない)。
        // 将来 --help 的な一覧を実装する場合、そちらには載せない。
        "--openapi" => {
            print_openapi();
        }
        "--create-user" => {
            let (username, is_admin) =
                parse_create_user_args(&args[2..]).unwrap_or_else(|message| fail(message));
            create_user(username, is_admin);
        }
        other => fail(format_args!("unknown option: {other}\n{USAGE}")),
    }
    true
}

/// `--create-user` に続く引数を、ユーザー名 (省略可) と管理者かどうかに分ける。
///
/// 未知の引数 (例: "--admin"の誤字) や余分な引数を黙って無視すると、意図せず一般ユーザーが
/// 作られたり、タイポがユーザー名になったりする。そのため次の4形式だけを許可し、
/// `-` で始まる語はユーザー名として受け取らない。
/// `[]` / `[--admin]` / `[<username>]` / `[<username>, --admin]`
fn parse_create_user_args(rest: &[String]) -> Result<(Option<&str>, bool), String> {
    let is_username = |value: &str| !value.starts_with('-');
    match rest {
        [] => Ok((None, false)),
        [flag] if flag == "--admin" => Ok((None, true)),
        [name] if is_username(name) => Ok((Some(name), false)),
        [name, flag] if is_username(name) && flag == "--admin" => Ok((Some(name), true)),
        _ => Err(format!(
            "unknown option(s): {}\n{CREATE_USER_USAGE}",
            rest.join(" ")
        )),
    }
}

/// OpenAPIスキーマをJSONで標準出力に書き出す。
/// `frontend/package.json` の `generate:api-types`(openapi-typescript)が
/// `../openapi.json` を読みに行くため、`weblav-cli --openapi > openapi.json` として使う想定。
/// 開発者・API連携者向けのフラグ(一般利用者向けの機能ではない)。
fn print_openapi() {
    let openapi = crate::api::openapi();
    match openapi.to_pretty_json() {
        Ok(json) => println!("{json}"),
        Err(err) => fail(format_args!("failed to write OpenAPI schema: {err}")),
    }
}

/// ユーザーを作成する。最初の管理者を作る手段の1つ (ほかはトレイから開くセットアップの画面、→ docs/access.md「初回セットアップ」)。
/// 管理者がいても作れるので、管理者のパスワードを忘れたときの戻り道にもなる。
/// ユーザー名を省いたら対話で聞く。
/// パスワードは引数や環境変数では受け取らない(シェル履歴やプロセス一覧に残るため)。
/// 実行時DB(`AppDirs::db_path()`)に対して行う。`DATABASE_URL` は参照しない
/// (開発環境では `just dev-cli` 経由なら `just dev-backend` と同じファイルを指す)。
/// `--admin` を付けると管理者ロールで作成する(既定は一般ユーザー)。
fn create_user(username: Option<&str>, is_admin: bool) {
    let username = match username {
        Some(username) => username.to_string(),
        None => prompt_username(),
    };
    let username = username.as_str();
    let role = if is_admin {
        crate::auth::Role::Admin
    } else {
        crate::auth::Role::User
    };

    let password = rpassword::prompt_password("Password: ")
        .unwrap_or_else(|err| fail(format_args!("failed to read password: {err}")));
    if password.is_empty() {
        fail("password must not be empty");
    }
    let confirm = rpassword::prompt_password("Password (confirm): ")
        .unwrap_or_else(|err| fail(format_args!("failed to read password: {err}")));
    if confirm != password {
        fail("passwords do not match");
    }

    let dirs = AppDirs::resolve().unwrap_or_else(|err| fail(error_chain(&err)));
    let db_path = dirs.db_path();

    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
    // DB接続・マイグレーションはユーザー作成そのものとエラー型が異なる(crate::db::Error)ため、
    // ここで先に済ませておく。接続の失敗は原因 (権限・パス) まで出さないと直しようがない。
    let pool = rt
        .block_on(async {
            let pool = crate::db::connect(&db_path).await?;
            crate::db::migrate(&pool).await?;
            Ok::<_, crate::db::Error>(pool)
        })
        .unwrap_or_else(|err| fail(format_args!("failed to create user: {}", error_chain(&err))));

    match rt.block_on(crate::auth::create_user(
        &pool, username, &password, role, None,
    )) {
        Ok(_) => {
            let role_label = if is_admin { "admin" } else { "user" };
            println!("created user '{username}' ({role_label})");
        }
        Err(crate::auth::CreateUserError::UsernameTaken(_)) => {
            fail(format_args!("user '{username}' already exists"))
        }
        Err(err) => fail(format_args!("failed to create user: {}", error_chain(&err))),
    }
}

/// ユーザー名を標準入力から1行読む。空なら終える。
fn prompt_username() -> String {
    print!("Username: ");
    // `print!` は改行が無いと書き出されないことがあり、入力待ちの前に促しが出ない。
    std::io::stdout()
        .flush()
        .unwrap_or_else(|err| fail(format_args!("failed to write prompt: {err}")));
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .unwrap_or_else(|err| fail(format_args!("failed to read username: {err}")));
    let username = line.trim();
    if username.is_empty() {
        fail("username must not be empty");
    }
    username.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn create_user_args_allow_omitting_the_username() {
        assert_eq!(parse_create_user_args(&args(&[])), Ok((None, false)));
        assert_eq!(
            parse_create_user_args(&args(&["--admin"])),
            Ok((None, true))
        );
    }

    #[test]
    fn create_user_args_accept_a_username_with_or_without_admin() {
        assert_eq!(
            parse_create_user_args(&args(&["satoh"])),
            Ok((Some("satoh"), false))
        );
        assert_eq!(
            parse_create_user_args(&args(&["satoh", "--admin"])),
            Ok((Some("satoh"), true))
        );
    }

    /// "--admin" の誤字をユーザー名として受け取ると、一般ユーザーが作られてしまう。
    #[test]
    fn create_user_args_reject_unknown_options_instead_of_using_them_as_username() {
        assert!(parse_create_user_args(&args(&["--admni"])).is_err());
        assert!(parse_create_user_args(&args(&["satoh", "--admni"])).is_err());
        assert!(parse_create_user_args(&args(&["satoh", "--admin", "extra"])).is_err());
    }
}
