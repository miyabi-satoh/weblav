//! 他の端末から開くためのアドレス(→ docs/architecture.md「LAN からの到達性」)。
//!
//! DHCP で変わりうる IP アドレスではなく、OS が名乗っている mDNS のホスト名を返す。同じ LAN にいなければ
//! そもそも開けないアドレスであり秘密ではないため、認証は掛けない(共通ヘッダーから
//! 誰でも参照できる)。

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ConnectionInfoResponse {
    /// mDNS のホスト名(`my-pc.local`等、ポートを除く)。全インターフェースで待ち受けて
    /// いないときや、OS から名前を読めなかったときは`None`。
    mdns_hostname: Option<String>,
    /// サーバーの待ち受けポート。`mdnsHostname`とあわせて使う。
    /// フロントエンドが開いている画面自身のポート(`location.port`)を使わないのは、
    /// `pnpm run dev` (Vite) のように画面とAPIサーバーのポートが別れる場合があるため。
    /// 全インターフェースで待ち受けていない (他の端末から開けない) ときは0。
    port: u16,
}

#[utoipa::path(
    get,
    path = "/connection-info",
    responses((status = OK, body = ConnectionInfoResponse))
)]
async fn connection_info(State(state): State<AppState>) -> Json<ConnectionInfoResponse> {
    Json(ConnectionInfoResponse {
        mdns_hostname: state.lan_port.and_then(|_| crate::mdns::os_hostname()),
        port: state.lan_port.unwrap_or(0),
    })
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(connection_info))
}
