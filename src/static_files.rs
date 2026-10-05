use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::{Embed, EmbeddedFile};

/// SvelteKit(SPA) のビルド成果物を埋め込む。
/// `frontend/build` は Cargo.toml からの相対パス。ディレクトリ自体が無いとコンパイルできない
/// ため、`cargo build`/`check`/`clippy`/`test` の前に `just frontend-build` 等で生成しておくこと
/// (`just dev-backend`・`just lint`・`just check`・`just test` は依存関係として自動実行する)。
#[derive(Embed)]
#[folder = "frontend/build"]
struct Assets;

const INDEX_HTML: &str = "index.html";

/// API以外の全リクエストを受けるフォールバックハンドラ。
/// - 埋め込みアセットに一致するパスがあればそれを返す
/// - 一致しなければ SPA のエントリーポイントとして `index.html` を返す(クライアントサイド
///   ルーティング用)
pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    match Assets::get(path) {
        Some(file) => serve(path, file),
        None => match Assets::get(INDEX_HTML) {
            Some(file) => serve(INDEX_HTML, file),
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

fn serve(path: &str, file: EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // content-hash付きファイル(_app/immutable/配下)は恒久キャッシュ可能、
    // index.html等は更新をすぐ反映したいのでキャッシュさせない。
    let cache_control = if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    (
        [
            (header::CONTENT_TYPE, mime.as_ref().to_string()),
            (header::CACHE_CONTROL, cache_control.to_string()),
        ],
        file.data.into_owned(),
    )
        .into_response()
}
