//! Windows向けにexeのバージョン情報・アイコンを埋め込む (→ docs/distribution.md)。
//!
//! `winresource`はcfg(windows)専用のbuild-dependency (Cargo.toml参照) のため、
//! 他のOS (開発機のmacOS等) ではこの関数の中身はコンパイル対象にならない。
//! `assets/icon.ico`の生成は `pnpm generate:icon` (scripts/generate-icon.mjs) で行う。

fn main() {
    // `winresource`自身はrerun-if-changedを発行しないため、指定しないとCargoの既定
    // (クレート内の何かが変わるたびにbuild.rsを再実行) に落ちてrc.exeが毎回走る。
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile()
            .expect("failed to embed Windows resources (icon/version info)");
    }
}
