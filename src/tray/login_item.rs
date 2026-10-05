//! トレイの「ログイン時に起動」(→ docs/distribution.md「常駐 (Windows)」)。オン・オフは weblav の設定に持たず、
//! 読むたびに OS の実際の状態を見る。
//! - macOS: `SMAppService` で .app をログイン項目に登録する (macOS 13 以降)。
//! - Windows: MSIX のマニフェストの `StartupTask` を切り替える。パッケージの外 (開発中の exe) では項目を出さない。
//! - Linux: XDG Autostart の `.desktop` ファイルを作る・消す。

/// OS の実際の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// 対応していない OS では項目を出さないので、どれも作らない。
#[cfg_attr(
    not(any(windows, target_os = "macos", target_os = "linux")),
    allow(dead_code)
)]
pub enum State {
    On,
    Off,
    /// ポリシーでオンにされていて、アプリからは止められない (Windows)。
    #[cfg_attr(not(windows), allow(dead_code))]
    OnLocked,
    /// 利用者が「設定」やタスクマネージャーで止めたか、ポリシーで止められていて、
    /// アプリからは戻せない (Windows)。
    #[cfg_attr(not(windows), allow(dead_code))]
    OffLocked,
    /// 登録はしたが、システム設定で利用者の許可が要る (macOS)。押すとシステム設定を開く。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NeedsApproval,
}

impl State {
    pub fn checked(self) -> bool {
        matches!(self, State::On | State::OnLocked)
    }

    pub fn changeable(self) -> bool {
        !matches!(self, State::OnLocked | State::OffLocked)
    }
}

pub use platform::{open_settings, set, state, supported};

#[cfg(target_os = "macos")]
mod platform {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    use super::State;

    /// `SMAppService` は macOS 13 から。それより前 (Info.plist の下限は 11) では項目を出さない。
    pub fn supported() -> bool {
        objc2::available!(macos = 13.0)
    }

    pub fn state() -> Result<State, String> {
        let status = unsafe { SMAppService::mainAppService().status() };
        Ok(match status {
            SMAppServiceStatus::Enabled => State::On,
            SMAppServiceStatus::RequiresApproval => State::NeedsApproval,
            // 一度も登録していない .app でも NotRegistered でなく NotFound が返る (実測) ので、
            // どちらもオフとして扱う。
            _ => State::Off,
        })
    }

    pub fn set(on: bool) -> Result<(), String> {
        let service = unsafe { SMAppService::mainAppService() };
        let result = if on {
            unsafe { service.registerAndReturnError() }
        } else {
            unsafe { service.unregisterAndReturnError() }
        };
        result.map_err(|err| format!("{err:?}"))
    }

    pub fn open_settings() {
        unsafe { SMAppService::openSystemSettingsLoginItems() }
    }
}

#[cfg(windows)]
mod platform {
    use windows::ApplicationModel::{StartupTask, StartupTaskState};
    use windows::core::h;

    use super::State;

    /// `StartupTask` はパッケージの中でしか使えない。
    pub fn supported() -> bool {
        packaged()
    }

    /// `installer/msix/AppxManifest.xml` の `StartupTask` の `TaskId` と揃える。
    fn task() -> windows::core::Result<StartupTask> {
        StartupTask::GetAsync(h!("WebLAV"))?.join()
    }

    pub fn state() -> Result<State, String> {
        let state = task()
            .and_then(|task| task.State())
            .map_err(|err| err.to_string())?;
        Ok(match state {
            StartupTaskState::Enabled => State::On,
            StartupTaskState::EnabledByPolicy => State::OnLocked,
            StartupTaskState::Disabled => State::Off,
            // DisabledByUser・DisabledByPolicy。利用者の選択はアプリから上書きできない。
            _ => State::OffLocked,
        })
    }

    pub fn set(on: bool) -> Result<(), String> {
        let task = task().map_err(|err| err.to_string())?;
        let result = if on {
            // パッケージのデスクトップ アプリからは、同意のダイアログを出さずに有効になる。
            // 有効にできなかったときは、読み直した状態でメニューに出る。
            task.RequestEnableAsync()
                .and_then(|op| op.join())
                .map(|_| ())
        } else {
            task.Disable()
        };
        result.map_err(|err| err.to_string())
    }

    /// Windows では `NeedsApproval` にならないので呼ばれない。
    pub fn open_settings() {}

    /// MSIX のパッケージの中で動いているか (→ docs/distribution.md「常駐 (Windows)」)。
    fn packaged() -> bool {
        use windows::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
        use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

        let mut len = 0;
        unsafe { GetCurrentPackageFullName(&mut len, None) != APPMODEL_ERROR_NO_PACKAGE }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::PathBuf;

    use super::{State, desktop_entry};

    pub fn supported() -> bool {
        true
    }

    /// `$XDG_CONFIG_HOME/autostart/weblav.desktop` (既定は `~/.config/autostart`)。
    fn entry_path() -> Result<PathBuf, String> {
        let dirs = directories::BaseDirs::new().ok_or("home directory not found")?;
        Ok(dirs.config_dir().join("autostart").join("weblav.desktop"))
    }

    pub fn state() -> Result<State, String> {
        match std::fs::read_to_string(entry_path()?) {
            Ok(text) if desktop_entry::enabled(&text) => Ok(State::On),
            Ok(_) => Ok(State::Off),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(State::Off),
            Err(err) => Err(err.to_string()),
        }
    }

    /// 起動の登録を消す。もともと無ければ、消せたのと同じに扱う。
    fn remove_entry(path: &std::path::Path) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.to_string()),
            _ => Ok(()),
        }
    }

    /// デスクトップの設定で止めたもの (`Hidden`・`X-GNOME-Autostart-enabled`) も、
    /// 作り直せば有効に戻せるので、Windows のように押せなくはしない。
    pub fn set(on: bool) -> Result<(), String> {
        let path = entry_path()?;
        if !on {
            return remove_entry(&path);
        }
        let exe = std::env::current_exe().map_err(|err| err.to_string())?;
        // 動いている間に更新で置き換えられると、末尾に ` (deleted)` の付いた無いパスが返る。
        if !exe.exists() {
            return Err(format!("executable not found: {}", exe.display()));
        }
        let exe = exe.to_str().ok_or("executable path is not UTF-8")?;
        // 改行は引用しても1行の値に収まらない。
        if exe.contains(['\n', '\r']) {
            return Err("executable path contains a line break".to_string());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        }
        // 書いてすぐ電源が落ちると、中身の無いファイルが残り、デスクトップが読めずに起動しなくなる。
        let tmp = path.with_extension("desktop.tmp");
        weblav::config::replace_owner_only_file(
            &tmp,
            &path,
            desktop_entry::render(exe).as_bytes(),
            false,
        )
        .map_err(|err| err.to_string())
    }

    /// Linux では `NeedsApproval` にならないので呼ばれない。
    pub fn open_settings() {}
}

/// XDG Autostart の `.desktop` ファイル (Desktop Entry Specification) の読み書き。
#[cfg(any(target_os = "linux", test))]
mod desktop_entry {
    /// 起動するファイルのパスを `Exec` に入れて、エントリの全文を作る。
    pub fn render(exe: &str) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName=WebLAV\nExec={}\nTerminal=false\n",
            quote_exec_arg(exe)
        )
    }

    /// `Exec` の引数の引用。空白などを含むパスでも1つの引数になるよう、常に二重引用符で囲み、
    /// 中の `"` `` ` `` `$` `\` を `\` で逃がす。`%` は `%%` にする (フィールドコードと区別するため)。
    /// さらに `.desktop` の文字列値としての `\` の逃がしが掛かるので、`\` は2重に書く。
    fn quote_exec_arg(arg: &str) -> String {
        let mut quoted = String::from("\"");
        for c in arg.chars() {
            match c {
                '"' | '`' | '$' => {
                    quoted.push_str("\\\\");
                    quoted.push(c);
                }
                '\\' => quoted.push_str("\\\\\\\\"),
                '%' => quoted.push_str("%%"),
                _ => quoted.push(c),
            }
        }
        quoted.push('"');
        quoted
    }

    /// デスクトップの設定で止められていないか。`Hidden=true` は仕様で「消したもの」とみなす印、
    /// `X-GNOME-Autostart-enabled=false` は GNOME の「自動起動するアプリ」で止めたときの印。
    /// `Type=Application` と `Exec` の無いもの (書きかけで残った空のファイルなど) は、
    /// デスクトップが起動できないので有効としない。
    pub fn enabled(text: &str) -> bool {
        let mut is_application = false;
        let mut has_exec = false;
        let mut in_main_group = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_main_group = line == "[Desktop Entry]";
                continue;
            }
            if !in_main_group {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match (key.trim(), value.trim()) {
                ("Hidden", "true") | ("X-GNOME-Autostart-enabled", "false") => return false,
                ("Type", "Application") => is_application = true,
                ("Exec", exec) if !exec.is_empty() => has_exec = true,
                _ => {}
            }
        }
        is_application && has_exec
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use super::State;

    pub fn supported() -> bool {
        false
    }

    pub fn state() -> Result<State, String> {
        Err("login item is not supported on this OS".to_string())
    }

    pub fn set(_on: bool) -> Result<(), String> {
        Err("login item is not supported on this OS".to_string())
    }

    pub fn open_settings() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// アプリから変えられない状態でも、チェックは OS の実際のオン・オフを映す。
    #[test]
    fn locked_states_keep_the_actual_check_but_cannot_be_changed() {
        assert!(State::OnLocked.checked());
        assert!(!State::OnLocked.changeable());
        assert!(!State::OffLocked.checked());
        assert!(!State::OffLocked.changeable());
    }

    #[test]
    fn desktop_entry_quotes_the_executable_path() {
        assert_eq!(
            desktop_entry::render("/opt/Web LAV/weblav"),
            "[Desktop Entry]\nType=Application\nName=WebLAV\nExec=\"/opt/Web LAV/weblav\"\nTerminal=false\n"
        );
        // `$` は Exec の引用で `\$`、`.desktop` の文字列値で `\\$` になる。`%` は `%%`。
        assert!(desktop_entry::render("/a$b%c").contains(r#"Exec="/a\\$b%%c""#));
        assert!(desktop_entry::render(r"/a\b").contains(r#"Exec="/a\\\\b""#));
    }

    #[test]
    fn desktop_entry_turned_off_by_the_desktop_is_not_enabled() {
        let base = "[Desktop Entry]\nType=Application\nExec=weblav\n";
        assert!(desktop_entry::enabled(base));
        assert!(!desktop_entry::enabled(&format!("{base}Hidden=true\n")));
        assert!(!desktop_entry::enabled(&format!(
            "{base}X-GNOME-Autostart-enabled=false\n"
        )));
        assert!(desktop_entry::enabled(&format!(
            "{base}X-GNOME-Autostart-enabled=true\n"
        )));
        // 書きかけで電源が落ちたときの空のファイル。
        assert!(!desktop_entry::enabled(""));
        // 起動に要るキーが欠けたもの。
        assert!(!desktop_entry::enabled("[Desktop Entry]\n"));
        assert!(!desktop_entry::enabled(
            "[Desktop Entry]\nType=Application\n"
        ));
        assert!(!desktop_entry::enabled("[Desktop Entry]\nExec=weblav\n"));
        assert!(!desktop_entry::enabled(
            "[Desktop Entry]\nType=Application\n[Desktop Action x]\nExec=weblav\n"
        ));
        // ほかのグループ (アクション) の同じキーは見ない。
        assert!(desktop_entry::enabled(&format!(
            "{base}[Desktop Action x]\nHidden=true\n"
        )));
    }

    /// 許可待ちはまだ起動の対象でないのでチェックを外し、押せば許可の場所へ案内する。
    #[test]
    fn needs_approval_is_unchecked_but_clickable() {
        assert!(!State::NeedsApproval.checked());
        assert!(State::NeedsApproval.changeable());
    }
}
