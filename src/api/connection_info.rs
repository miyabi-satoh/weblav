//! 他の端末から開くためのアドレス(→ docs/architecture.md「LAN からの到達性」)。
//!
//! 案内の主は、DHCP で変わりうる IP アドレスではなく、OS が名乗っている mDNS のホスト名。
//! 名前を引けない端末のために、LAN の IPv4 アドレスも添える (→ docs/ui.md「ページの共有」)。同じ LAN にいなければ
//! そもそも開けないアドレスであり秘密ではないため、認証は掛けない(共通ヘッダーから
//! 誰でも参照できる)。

use std::net::Ipv4Addr;

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{AppError, run_blocking};
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
    /// この PC の LAN の IPv4 アドレス (`192.168.1.13`等)。`mdnsHostname`の名前を引けない端末で開くため。
    /// 仮想の LAN (Docker・VPN など) のものは除く。全インターフェースで待ち受けていないときは空。
    #[schema(value_type = Vec<String>)]
    lan_addresses: Vec<Ipv4Addr>,
}

#[utoipa::path(
    get,
    path = "/connection-info",
    responses((status = OK, description = "他の端末から開くためのアドレス", body = ConnectionInfoResponse))
)]
async fn connection_info(
    State(state): State<AppState>,
) -> Result<Json<ConnectionInfoResponse>, AppError> {
    let Some(port) = state.lan_port else {
        return Ok(Json(ConnectionInfoResponse {
            mdns_hostname: None,
            port: 0,
            lan_addresses: Vec::new(),
        }));
    };
    let (mdns_hostname, lan_addresses) =
        run_blocking(|| (crate::mdns::os_hostname(), lan_ipv4_addresses())).await?;
    Ok(Json(ConnectionInfoResponse {
        mdns_hostname,
        port,
        lan_addresses,
    }))
}

/// この PC の LAN の IPv4 アドレス。読めなければ空 (名前での案内は残るので、エラーにしない)。
fn lan_ipv4_addresses() -> Vec<Ipv4Addr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut addresses: Vec<Ipv4Addr> = interfaces
        .into_iter()
        .filter(|interface| interface.is_oper_up() && !interface.is_p2p())
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(v4) if is_lan_candidate(&interface.name, v4.ip) => Some(v4.ip),
            _ => None,
        })
        .collect();
    addresses.sort();
    addresses.dedup();
    addresses
}

/// ほかの端末に案内してよい LAN のアドレスか。
///
/// プライベートのアドレスに限るのは、学校や家庭の LAN はこの範囲で、公開のアドレスや
/// VPN (Tailscale の 100.64.0.0/10 など) は同じ Wi-Fi の端末から届かないため。
/// Docker・仮想マシン・VPN の仮想の LAN もプライベートのアドレスを持つので、名前で除く。
fn is_lan_candidate(interface_name: &str, ip: Ipv4Addr) -> bool {
    // macOS・Linux のインターフェースの名前 (小文字)。大文字と小文字を区別して比べるのは、
    // Windows の実の LAN につながる `vEthernet (…)` を `veth` に当てないため。
    // `bridge` は macOS のインターネット共有と仮想マシン (`bridge100` など)。Linux で実の LAN を
    // 束ねるブリッジは `br0` などの名前なので当たらない。
    const VIRTUAL_PREFIXES: [&str; 19] = [
        "docker",
        "br-",
        "veth",
        "virbr",
        "vmnet",
        "vboxnet",
        "vnic",
        "bridge",
        "lxcbr",
        "lxdbr",
        "incusbr",
        "cni",
        "podman",
        "utun",
        "tun",
        "tap",
        "wg",
        "tailscale",
        "zt",
    ];
    // Windows の表示名 (if-addrs は Windows では FriendlyName を名前にする)。WSL と Hyper-V の既定の
    // スイッチ、VMware・VirtualBox のホストの LAN。Hyper-V の外部のスイッチ (実の LAN) は別の名前になる。
    const VIRTUAL_WINDOWS_MARKS: [&str; 4] = ["(WSL", "(Default Switch)", "VMware", "VirtualBox"];
    ip.is_private()
        && !VIRTUAL_PREFIXES
            .iter()
            .any(|prefix| interface_name.starts_with(prefix))
        && !VIRTUAL_WINDOWS_MARKS
            .iter()
            .any(|mark| interface_name.contains(mark))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(connection_info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_address_on_a_real_interface_is_offered() {
        for (name, ip) in [
            ("en0", [192, 168, 1, 13]),
            ("wlan0", [10, 0, 0, 5]),
            ("Wi-Fi", [172, 20, 1, 2]),
            ("vEthernet (External)", [192, 168, 0, 8]),
        ] {
            assert!(is_lan_candidate(name, Ipv4Addr::from(ip)), "{name}");
        }
    }

    #[test]
    fn public_vpn_and_link_local_addresses_are_not_offered() {
        for ip in [
            [8, 8, 8, 8],
            [100, 101, 1, 2],
            [169, 254, 3, 4],
            [127, 0, 0, 1],
        ] {
            assert!(!is_lan_candidate("en0", Ipv4Addr::from(ip)), "{ip:?}");
        }
    }

    #[test]
    fn virtual_interfaces_are_not_offered() {
        for name in [
            "docker0",
            "br-1a2b3c",
            "veth12ab",
            "virbr0",
            "vmnet8",
            "vboxnet0",
            "utun3",
            "tun0",
            "wg0",
            "tailscale0",
            "vEthernet (WSL (Hyper-V firewall))",
            "vEthernet (Default Switch)",
            "VMware Network Adapter VMnet8",
            "VirtualBox Host-Only Network",
            "bridge100",
            "vnic0",
            "lxdbr0",
            "podman0",
        ] {
            assert!(
                !is_lan_candidate(name, Ipv4Addr::new(172, 17, 0, 1)),
                "{name}"
            );
        }
    }
}
