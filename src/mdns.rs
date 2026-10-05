//! 他の端末に案内する、この PC の mDNS のホスト名 (`<名前>.local`) (→ docs/architecture.md「LAN からの到達性」)。
//!
//! OS が名乗っている名前を読んで案内するだけで、WebLAV 自身は名乗らない。
//! OS (macOS の mDNSResponder・Windows・Linux の Avahi) はすでにその名前で応えており、
//! 重ねて名乗ると OS が衝突とみなして、PC の名前を付け直してしまうため。
//! macOS は衝突で名前を付け直すことがあるので、起動時に決めず、案内するたびに読む。

/// 今この PC が mDNS で名乗っている名前 (`.local` を含む、例: `my-pc.local`)。
/// 読めなければ `None`。
#[cfg(target_os = "macos")]
pub fn os_hostname() -> Option<String> {
    use objc2_system_configuration::SCDynamicStore;

    // Bonjour の名前は「共有」のローカルホスト名 (LocalHostName)。`gethostname` は
    // DHCP で配られた名前などを返すことがあり、Bonjour の名前と一致するとはかぎらない。
    let name = SCDynamicStore::local_host_name(None)?.to_string();
    (!name.is_empty()).then(|| format!("{name}.local"))
}

/// 今この PC が mDNS で名乗っている名前 (`.local` を含む、例: `desktop-abc123.local`)。
/// 読めなければ `None`。
#[cfg(not(target_os = "macos"))]
pub fn os_hostname() -> Option<String> {
    let label = hostname_label(&gethostname::gethostname().to_string_lossy())?;
    Some(format!("{label}.local"))
}

/// OS のコンピュータ名から、OS が mDNS で名乗るホスト名ラベルを求める。
///
/// Windows と Avahi は、コンピュータ名のドメイン部分を除いたものを `.local` の前に付けて名乗る。
/// `gethostname` は環境により FQDN を返すのでドメイン部分は捨てる。英数字とハイフン以外が
/// 混じるときは `None`。別の名前に直すと、誰も応えない名前を案内することになるため。
#[cfg(any(not(target_os = "macos"), test))]
fn hostname_label(raw: &str) -> Option<String> {
    let label = raw.split('.').next().unwrap_or("").to_ascii_lowercase();
    let valid = !label.is_empty()
        && label.len() <= 63
        && label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    valid.then_some(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_label_lowercases_and_keeps_hyphens() {
        assert_eq!(
            hostname_label("DESKTOP-ABC123").as_deref(),
            Some("desktop-abc123")
        );
    }

    /// FQDN で返る環境 (Linuxの一部設定等) では、ドメイン部分を含めない。
    #[test]
    fn hostname_label_strips_domain_suffix() {
        assert_eq!(hostname_label("host.example.com").as_deref(), Some("host"));
    }

    /// OS が名乗る名前と食い違うので、作り変えずに案内しない。
    #[test]
    fn hostname_label_is_none_for_names_outside_dns_label() {
        assert_eq!(hostname_label("my_pc"), None);
        assert_eq!(hostname_label("事務所PC"), None);
        assert_eq!(hostname_label(""), None);
        assert_eq!(hostname_label("..."), None);
        assert_eq!(hostname_label(&"a".repeat(64)), None);
    }

    /// 読めた名前は `.local` で終わる、DNS ラベル1つの形になっている。
    #[test]
    fn os_hostname_is_a_local_name() {
        let Some(name) = os_hostname() else { return };
        let label = name.strip_suffix(".local").expect("ends with .local");
        assert!(!label.is_empty() && !label.contains('.'), "{name}");
    }
}
