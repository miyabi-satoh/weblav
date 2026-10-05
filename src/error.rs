use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use utoipa::ToSchema;

/// エラーとその原因(source)チェーンを1つの文字列に組み立てる。
/// `thiserror` の `#[error(...)]` は最上位のメッセージしか出さないため、
/// `toml::de::Error` が持つ行番号やDB接続の失敗理由等の詳細を落とさないように辿る。
/// 本体の起動失敗と `weblav-cli` の両方で使う。
pub fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut chain = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        chain.push_str("\ncaused by: ");
        chain.push_str(&err.to_string());
        source = err.source();
    }
    chain
}

/// API全体で共通のエラー型。バリアントは実際にそれを生成する箇所ができた時点で追加する
/// (投機的に増やさない)。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("unauthorized")]
    Unauthorized,
    /// ログイン済みだが権限(role)が不足している場合。未ログインの`Unauthorized`とは
    /// 区別する: frontendが「ログイン画面に誘導する」か「権限がない旨を伝える」かを
    /// 区別できるようにするため。
    #[error("forbidden")]
    Forbidden,
    #[error("invalid credentials")]
    InvalidCredentials,
    /// 自分のパスワードを変更する際、入力した現在のパスワードが違う場合。ログイン失敗の
    /// `InvalidCredentials`とは別コードにする(あちらの文言は「ユーザー名またはパスワード」
    /// で、パスワード変更ダイアログにはユーザー名の欄が無いため)。
    #[error("incorrect current password")]
    IncorrectCurrentPassword,
    /// リカバリコードでの再設定に失敗した場合 (→ docs/access.md「リカバリコード」)。ユーザーがいない・
    /// コードが無い・違う、を区別しない。
    #[error("invalid recovery code")]
    InvalidRecoveryCode,
    #[error("too many requests")]
    TooManyRequests,
    /// リクエストボディが不正(JSONとして解釈できない・必須フィールド欠落・
    /// `Content-Type` が `application/json` でない等)。`axum::Json` の rejection を
    /// そのまま返すと共通envelopeにならないため、`AppJson` 経由でここに変換する。
    #[error("{message}")]
    InvalidJson { status: StatusCode, message: String },
    /// リクエストボディはJSONとして正しいが、値が業務ルールを満たさない場合
    /// (タイトルが空・URLのスキームがhttp(s)でない等)。`InvalidJson`は
    /// デシリアライズ自体の失敗用なのでこちらは別バリアントにする。
    /// codeは`InvalidJson`と同じ`invalid_request_body`にまとめる
    /// (frontend/利用者から見ればどちらも「入力内容を確認してください」で変わらないため)。
    #[error("{0}")]
    Validation(String),
    /// `Validation` のうち、画面が具体的な文言を組み立てられるよう内訳を添えるもの
    /// (→ `api::error_detail`)。`message` は英語のデバッグ用で、表示には使わない。
    #[error("{message}")]
    ValidationDetailed {
        message: String,
        detail: crate::api::error_detail::ValidationDetail,
    },
    /// パスワードハッシュの検証自体が失敗した場合(DB内のハッシュが壊れている等)。
    /// 「パスワードが違う」とは区別する: こちらはクライアント起因ではないため5xx。
    #[error("internal error")]
    Internal(#[from] crate::auth::Error),
    /// アプリケーションが前提とする不変条件が崩れている場合(例: `type = 'link'` の行に
    /// `url` が無い)。書き込み経路が正しければ起こらないはずだが、クライアント起因の
    /// 4xxとして握り潰すより5xxで検知できるようにしておく。
    #[error("data integrity error: {0}")]
    DataIntegrity(&'static str),
    /// folder コンテンツのファイル配信・一覧取得中に発生したファイルシステムエラー。
    /// 「パスが存在しない/権限が無い」等はパストラバーサル対策と合わせて呼び出し側で
    /// `NotFound` に変換するため、ここに来るのはそれ以外の想定外のI/Oエラー(クライアント
    /// 起因ではないため5xx)。
    #[error("io error")]
    Io(#[from] std::io::Error),
    /// `axum::extract::Multipart` の読み取り中に発生したエラー(不正なmultipart構造・
    /// フィールド単体のサイズ超過等)。`AppJson`のJSON rejection変換
    /// (`json_rejection_to_app_error`)と同じ方針で、ここで共通envelopeに変換する。
    #[error("{message}")]
    InvalidMultipart { status: StatusCode, message: String },
    /// 同じ資源に対する操作が既に走っている場合 (アーカイブの走査が二重に要求された等)。
    /// クライアントは時間を置いて再試行すればよく、入力自体は正しいため
    /// `Validation` とは区別する。
    #[error("{0}")]
    Conflict(String),
    /// `config.toml` が読めない形になっている場合 (起動した後に手で書き換えた等)。
    /// 管理画面から書き換えると手で書いた内容を消すので、直してもらうよう伝える
    /// (→ `api::server_settings`)。
    #[error("{0}")]
    ConfigUnreadable(String),
    /// アップロードされたファイルが`config.upload.max_size_mb`(`AppState::max_upload_bytes`)を
    /// 超えた場合。ストリーミング中に自前でカウントして検知する(`InvalidMultipart`の
    /// サイズ超過はサーバー全体の受付上限`DefaultBodyLimit`側の話で、こちらは
    /// アプリケーションが定める実際の上限)。
    #[error("file too large")]
    FileTooLarge,
    /// Free の上限に当たった (→ docs/pro.md「上限を数えて止める」)。入力は正しく、Pro にすれば通るため
    /// `Validation` とは分ける。
    #[error("free limit reached")]
    FreeLimitReached {
        target: crate::api::free_limit::FreeLimitTarget,
        limit: i64,
    },
    /// Pro の窓口 (→ docs/pro.md「アカウントと販売の窓口」) とやり取りできない (つながらない・
    /// 応答が読めない・証明を確かめられない)。
    #[error("account server error: {0}")]
    AccountServer(String),
    /// 打ち込まれた返しのコードが、結びかけの秘密で確かめられない (打ち間違い・別の申し込みのコード・期限切れ)。
    #[error("invalid Pro code")]
    ProCodeInvalid,
    /// PC の時計が、返しのコードの発行日や前に見た時刻より遅れている (→ docs/pro.md「結び付きと許可」の「時計」)。
    /// 打ち間違いとは直し方が違う (時計を合わせる) ので分ける。
    #[error("the clock is behind")]
    ProClockBehind,
}

/// API エラーの機械可読なコード。frontend はこの値で表示文言を選ぶ。
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    DatabaseUnavailable,
    Unauthorized,
    Forbidden,
    InvalidCredentials,
    IncorrectCurrentPassword,
    InvalidRecoveryCode,
    TooManyRequests,
    InvalidRequestBody,
    FileTooLarge,
    InternalError,
    Conflict,
    ConfigUnreadable,
    FreeLimitReached,
    AccountServerUnavailable,
    ProCodeInvalid,
    ProClockBehind,
}

/// JSON に出る値と同じ文字列 (`not_found` など)。一覧を二重に持たないよう serde から作る。
impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.serialize(f)
    }
}

/// 返しのコードの打ち込みの失敗 (→ docs/pro.md「結ぶ・確かめる・外す (WebLAV の側)」)。
impl From<crate::pro::CodeError> for AppError {
    fn from(err: crate::pro::CodeError) -> Self {
        use crate::pro::CodeError;
        match err {
            CodeError::NotStarted => AppError::Conflict("no pending link".to_string()),
            CodeError::Invalid => AppError::ProCodeInvalid,
            CodeError::ClockBehind => AppError::ProClockBehind,
            CodeError::Write(err) => AppError::Io(err),
        }
    }
}

/// 窓口の答えを手元に合わせるときの失敗。証明が確かめられないのは窓口の側の不調として扱う。
impl From<crate::pro::ApplyError> for AppError {
    fn from(err: crate::pro::ApplyError) -> Self {
        use crate::pro::ApplyError;
        match err {
            ApplyError::Invalid(_) => AppError::AccountServer(error_chain(&err)),
            ApplyError::ClockBehind => AppError::ProClockBehind,
            ApplyError::Write(err) => AppError::Io(err),
        }
    }
}

/// 結びかけを作るときの失敗。
impl From<crate::pro::LinkError> for AppError {
    fn from(err: crate::pro::LinkError) -> Self {
        use crate::pro::LinkError;
        match err {
            LinkError::Write(err) => AppError::Io(err),
            err => AppError::AccountServer(error_chain(&err)),
        }
    }
}

/// ユーザー作成の失敗。ユーザー管理と初回セットアップで、画面に返す形を揃える。
impl From<crate::auth::CreateUserError> for AppError {
    fn from(err: crate::auth::CreateUserError) -> Self {
        use crate::auth::CreateUserError;
        match err {
            CreateUserError::UsernameTaken(name) => username_taken(name),
            CreateUserError::Db(err) => AppError::Database(err),
            CreateUserError::Hash(err) => AppError::Internal(err),
        }
    }
}

/// ユーザー名の変更の失敗。重なりは作成と同じ形で返す。
impl From<crate::auth::RenameUserError> for AppError {
    fn from(err: crate::auth::RenameUserError) -> Self {
        use crate::auth::RenameUserError;
        match err {
            RenameUserError::UsernameTaken(name) => username_taken(name),
            RenameUserError::NotFound => AppError::NotFound,
            RenameUserError::Db(err) => AppError::Database(err),
        }
    }
}

fn username_taken(name: String) -> AppError {
    AppError::ValidationDetailed {
        message: "username is already taken".to_string(),
        detail: crate::api::error_detail::ValidationDetail::UsernameTaken { name },
    }
}

impl AppError {
    fn status_and_code(&self) -> (StatusCode, ErrorCode) {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, ErrorCode::NotFound),
            Self::Database(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::DatabaseUnavailable,
            ),
            // ログイン前 / セッション切れ。「間違ったパスワード」とは別コードにして、
            // frontendが「ログイン画面に誘導する」か「パスワードが違うと伝える」かを
            // 区別できるようにする。
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized),
            Self::Forbidden => (StatusCode::FORBIDDEN, ErrorCode::Forbidden),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, ErrorCode::InvalidCredentials),
            Self::IncorrectCurrentPassword => (
                StatusCode::UNAUTHORIZED,
                ErrorCode::IncorrectCurrentPassword,
            ),
            Self::InvalidRecoveryCode => (StatusCode::UNAUTHORIZED, ErrorCode::InvalidRecoveryCode),
            Self::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, ErrorCode::TooManyRequests),
            Self::InvalidJson { status, .. } => (*status, ErrorCode::InvalidRequestBody),
            Self::Validation(_) | Self::ValidationDetailed { .. } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::InvalidRequestBody,
            ),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
            Self::DataIntegrity(_) => (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
            Self::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
            // ペイロード過大(413)は`file_too_large`にまとめる。それ以外のmultipart不正は
            // `invalid_request_body`(JSON不正と同じコード。frontendから見た意味は同じ)。
            Self::InvalidMultipart { status, .. } => {
                let code = if *status == StatusCode::PAYLOAD_TOO_LARGE {
                    ErrorCode::FileTooLarge
                } else {
                    ErrorCode::InvalidRequestBody
                };
                (*status, code)
            }
            Self::Conflict(_) => (StatusCode::CONFLICT, ErrorCode::Conflict),
            Self::ConfigUnreadable(_) => (StatusCode::CONFLICT, ErrorCode::ConfigUnreadable),
            Self::FileTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, ErrorCode::FileTooLarge),
            Self::FreeLimitReached { .. } => (StatusCode::CONFLICT, ErrorCode::FreeLimitReached),
            Self::AccountServer(_) => {
                (StatusCode::BAD_GATEWAY, ErrorCode::AccountServerUnavailable)
            }
            Self::ProCodeInvalid => (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::ProCodeInvalid),
            Self::ProClockBehind => (StatusCode::CONFLICT, ErrorCode::ProClockBehind),
        }
    }
}

/// レスポンスボディの共通envelope。`code` が機械可読な契約(frontendはこちらでi18nする)、
/// `message` は英語のデバッグ用の説明文で、画面には出さない。
///
/// 汎用の文言では直しようがない422には `detail` を添える。こちらも機械可読な契約で、
/// 画面が値を差し込んで文言を組み立てる (→ `api::error_detail`)。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    code: ErrorCode,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<crate::api::error_detail::ValidationDetail>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        if status.is_server_error() {
            // Debugでログに出す: `Database`バリアントの元の sqlx::Error まで残すため。
            // レスポンスボディ側(Display)はDB内部の詳細を漏らさない定型文のまま。
            tracing::error!(error = ?self, "API error");
        }
        let message = self.to_string();
        let detail = match self {
            Self::ValidationDetailed { detail, .. } => Some(detail),
            Self::FreeLimitReached { target, limit } => {
                Some(crate::api::error_detail::ValidationDetail::FreeLimit { target, limit })
            }
            _ => None,
        };
        let body = ErrorResponse {
            error: ErrorBody {
                code,
                message,
                detail,
            },
        };
        (status, Json(body)).into_response()
    }
}

/// `axum::Json` の代わりに使うJSON body エクストラクタ。
/// 抽出失敗(不正なJSON・必須フィールド欠落・`Content-Type` 不一致)を、素の
/// axumレスポンスではなく `AppError`(共通envelope)に変換する。
pub struct AppJson<T>(pub T);

impl<S, T> FromRequest<S> for AppJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => Err(json_rejection_to_app_error(rejection)),
        }
    }
}

fn json_rejection_to_app_error(rejection: JsonRejection) -> AppError {
    AppError::InvalidJson {
        status: rejection.status(),
        message: rejection.body_text(),
    }
}

/// ブロッキング処理を `spawn_blocking` に載せて完了を待つ。
/// タスク自体の失敗 (panic 等) はクライアント起因ではないので `Io` にまとめる。
pub(crate) async fn run_blocking<F, T>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| AppError::Io(std::io::Error::other(err)))
}

/// `axum::extract::multipart::MultipartError` を共通envelopeに変換する。
/// `json_rejection_to_app_error` と同じ方針(ライブラリ側のstatus/body_textをそのまま使う)。
pub fn multipart_error_to_app_error(err: axum::extract::multipart::MultipartError) -> AppError {
    AppError::InvalidMultipart {
        status: err.status(),
        message: err.body_text(),
    }
}
