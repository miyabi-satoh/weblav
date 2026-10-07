mod archive;
mod archive_axes;
mod archive_axis_hints;
mod archive_items;
mod auth;
pub mod backup;
mod browser;
mod completed_reader;
mod connection_info;
mod contents;
pub mod error_detail;
pub mod free_limit;
mod fs;
mod health;
mod help;
pub mod link_preview;
mod link_title;
mod links_file;
mod local;
mod logs;
pub mod pro;
mod remote_file;
mod roots;
mod search;
pub mod server_settings;
pub mod setup;
mod site_settings;
mod sort;
mod text_files;
pub mod thumbnails;
mod users;
mod validate;

use utoipa::openapi::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use crate::error::{AppError, ErrorCode, ErrorResponse};
use crate::state::AppState;

/// `N` バイトの乱数を16進の文字列にする。トークンや作業用の名前に使う。
/// 乱数を取れないのは OS 側の異常で、クライアント起因ではないため、呼び出し側では 5xx にする。
pub(crate) fn random_hex<const N: usize>() -> std::io::Result<String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(std::io::Error::other)?;
    Ok(hex_encode(&bytes))
}

/// バイト列を小文字16進文字列に変換する (この用途だけのために `hex` クレートを増やさない)。
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 外へつなぐときの TLS。学校や事務所のプロキシが差し込む証明書も通るよう、OS の証明書ストアを使う
/// (既定は同梱の Mozilla のもの)。native-tls は自動では選ばれないので明示する。
pub(crate) fn native_tls_config() -> ureq::tls::TlsConfig {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build()
}

/// OpenAPIドキュメントのベース(タイトル・スキーマ登録)。
/// `ErrorResponse` はどのハンドラの戻り値にも直接現れない(`AppError`経由でレスポンスに
/// なる)ため、明示的に登録しないとOpenAPIスキーマから漏れる。
#[derive(utoipa::OpenApi)]
#[openapi(info(title = "weblav"), components(schemas(ErrorResponse, ErrorCode)))]
struct ApiDoc;

/// `/api/v1` 配下のルーターを組み立てる(まだ `/api/v1` はネストしていない)。
/// `fallback` をここで設定しておくのが重要: axumの`nest`はネストするルーター自身が
/// fallbackを持つ場合はそれを引き継ぐため、`/api/v1/*` 配下の未知のパスが
/// SPAの`index.html`にフォールバックしてしまうのを防げる。
///
/// `max_upload_bytes` はファイルアップロード系エンドポイントの`DefaultBodyLimit`用
/// (`contents::router`参照)。
fn routes(max_upload_bytes: u64) -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(connection_info::router())
        .merge(contents::router(max_upload_bytes))
        .merge(archive::router())
        .merge(archive_axes::router())
        .merge(archive_axis_hints::router())
        .merge(archive_items::router())
        .merge(links_file::router())
        .merge(remote_file::router())
        .merge(fs::router())
        .merge(help::router())
        .merge(link_preview::router())
        .merge(roots::router())
        .merge(search::router())
        .merge(setup::router())
        .merge(logs::router())
        .merge(pro::router())
        .merge(backup::router())
        .merge(server_settings::router())
        .merge(site_settings::router())
        .merge(users::router())
        .fallback(|| async { AppError::NotFound })
}

/// `/api/v1` をnestしただけの、状態未確定のルーター。
/// `Router<AppState>` はまだ`AppState`の実体(DB接続プール)を必要とせず組み立てられる
/// (実体が要るのは`.with_state()`する時、あるいは実際にリクエストを捌く時)ため、
/// `--openapi` のようにDBに触れずOpenAPIドキュメントだけ欲しい場合はここで止めてよい。
pub(crate) fn routed(max_upload_bytes: u64) -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(<ApiDoc as utoipa::OpenApi>::openapi())
        .nest("/api/v1", routes(max_upload_bytes))
}

/// OpenAPIドキュメントを組み立てる。DBには一切触れない。
/// `DefaultBodyLimit`の具体的な値はOpenAPIスキーマに影響しないため、
/// `config::UploadConfig::default()`の値で組み立てて構わない。
pub fn openapi() -> OpenApi {
    let (_router, mut openapi) =
        routed(crate::config::UploadConfig::default().max_bytes()).split_for_parts();
    openapi.info.version = crate::APP_VERSION.to_string();
    // Cargo.toml に description/license を書いていないため、derive が生成した空文字列を
    // 空欄のまま出すよりは省いておく。
    openapi.info.description = None;
    openapi.info.license = None;
    fill_missing_response_descriptions(&mut openapi);
    openapi
}

/// `description` を書いていないレスポンスに、ステータスの名前 ("OK" など) を入れる。
/// OpenAPI 3.1 では Response の `description` は必須だが、utoipa 6 は空のとき出力を省く。
fn fill_missing_response_descriptions(openapi: &mut OpenApi) {
    use utoipa::openapi::RefOr;

    for item in openapi.paths.paths.values_mut() {
        let operations = [
            &mut item.get,
            &mut item.put,
            &mut item.post,
            &mut item.delete,
            &mut item.patch,
        ];
        for operation in operations.into_iter().flatten() {
            for (status, response) in &mut operation.responses.responses {
                if let RefOr::T(response) = response
                    && response.description.is_empty()
                {
                    response.description = status
                        .parse::<u16>()
                        .ok()
                        .and_then(|code| axum::http::StatusCode::from_u16(code).ok())
                        .and_then(|code| code.canonical_reason())
                        .unwrap_or(status)
                        .to_string();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_response_has_a_description() {
        let json = serde_json::to_value(openapi()).unwrap();
        for (path, item) in json["paths"].as_object().unwrap() {
            for (method, operation) in item.as_object().unwrap() {
                for (status, response) in operation["responses"].as_object().unwrap() {
                    assert!(
                        response["description"]
                            .as_str()
                            .is_some_and(|d| !d.is_empty()),
                        "{method} {path} {status} に description が無い"
                    );
                }
            }
        }
    }
}
