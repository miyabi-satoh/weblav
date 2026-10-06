//! Windows向けにexeのバージョン情報・アイコンを埋め込む (→ docs/distribution.md)。
//! ビルド番号 (`WEBLAV_BUILD`) も数えて渡す (→ docs/distribution.md「版の番号」)。
//!
//! `winresource`はcfg(windows)専用のbuild-dependency (Cargo.toml参照) のため、
//! 他のOS (開発機のmacOS等) ではこの関数の中身はコンパイル対象にならない。
//! `assets/icon.ico`の生成は `pnpm generate:icon` (scripts/generate-icon.mjs) で行う。

fn main() {
    // `winresource`自身はrerun-if-changedを発行しないため、指定しないとCargoの既定
    // (クレート内の何かが変わるたびにbuild.rsを再実行) に落ちてrc.exeが毎回走る。
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    emit_build_number();

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile()
            .expect("failed to embed Windows resources (icon/version info)");
    }
}

/// ビルド番号を、試しの MSIX の版番号の4つ目 (`scripts/msix.mjs`) と同じ数え方で渡す。
/// 同じコミットから作った MSIX と、画面に出る番号をそろえるため。
fn emit_build_number() {
    let Some(count) = git(&["rev-list", "--count", "HEAD"]) else {
        return;
    };
    println!("cargo:rustc-env=WEBLAV_BUILD={count}");

    // コミットが進んだら数え直す。HEAD の指す先 (ブランチの ref) と、ref が詰められた先の
    // packed-refs を見張る。無いファイルを指定すると Cargo は毎回 build.rs を走らせるので、
    // 有るものだけを渡す。worktree でも正しい場所を指すよう、場所は git に聞く。
    let mut watched = vec!["HEAD".to_string(), "packed-refs".to_string()];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(branch);
    }
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--git-path", &name])
            && std::path::Path::new(&path).exists()
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}
