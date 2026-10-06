//! サーバーの PC に OS 標準のフォルダ選択の窓を出す (→ docs/folders.md「選び方」)。
//!
//! macOS の窓 (NSOpenPanel) はメインスレッドでしか開けない。そのため窓を開く頼みはメインスレッドへ回し
//! (`on_main_thread`)、実行ファイルのメインスレッドが `PickRequest::serve` で応える。
//! トレイ版はトレイのイベントループで、`weblav-service` は停止を待つループで受ける。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

/// 窓を開く手段。窓は同時に1つだけ開く (`begin`)。
#[derive(Clone)]
pub struct FolderPicker {
    pick: Arc<dyn Fn() -> Result<Option<PathBuf>, Unavailable> + Send + Sync>,
    busy: Arc<AtomicBool>,
}

/// 窓を開けなかった (応える側がもう動いていない)。
#[derive(Debug)]
pub struct Unavailable;

impl FolderPicker {
    /// `pick` は窓を開いて閉じるまで待ち、選ばれたパス (キャンセルなら `None`) を返す。
    /// テストでは決まったパスを返すものを渡す。
    pub fn new(
        pick: impl Fn() -> Result<Option<PathBuf>, Unavailable> + Send + Sync + 'static,
    ) -> Self {
        Self {
            pick: Arc::new(pick),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 窓を開く番を取る。開いている窓があれば `None`。
    ///
    /// 番は `PickSession` を drop するまで (= 窓が閉じるまで) 持ち続ける。
    /// 要求した側が待つのをやめても、窓は PC の前に残っているため。
    pub fn begin(&self) -> Option<PickSession> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| PickSession {
                picker: self.clone(),
            })
    }
}

/// 窓を開く番。`FolderPicker::begin` で取る。
pub struct PickSession {
    picker: FolderPicker,
}

impl PickSession {
    /// 窓を開き、閉じるまで待つ。ブロックするので `spawn_blocking` の中で呼ぶ。
    pub fn pick(self) -> Result<Option<PathBuf>, Unavailable> {
        (self.picker.pick)()
    }
}

impl Drop for PickSession {
    fn drop(&mut self) {
        self.picker.busy.store(false, Ordering::Release);
    }
}

/// 窓を開く頼みをメインスレッドへ回す `FolderPicker` と、その頼みの受け口。
pub fn on_main_thread() -> (FolderPicker, PickRequests) {
    let (tx, rx) = mpsc::channel::<PickRequest>();
    let picker = FolderPicker::new(move || {
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(PickRequest(reply_tx)).map_err(|_| Unavailable)?;
        reply_rx.recv().map_err(|_| Unavailable)
    });
    (picker, PickRequests(rx))
}

/// メインスレッドへ回ってくる、窓を開く頼みの受け口。
pub struct PickRequests(mpsc::Receiver<PickRequest>);

impl PickRequests {
    /// 別のスレッドで頼みを待ち、届くたびに `send` で送り先へ流す。`send` が `false` を返したら
    /// (送り先が閉じたら)、または `FolderPicker` がすべて drop されたら、そのスレッドを終える。
    pub fn forward(self, mut send: impl FnMut(PickRequest) -> bool + Send + 'static) {
        std::thread::spawn(move || {
            while let Ok(request) = self.0.recv() {
                if !send(request) {
                    break;
                }
            }
        });
    }
}

/// 窓を開く頼み1件。
pub struct PickRequest(mpsc::Sender<Option<PathBuf>>);

impl PickRequest {
    /// 窓を開き、選ばれたパスを頼んだ側へ返す。**メインスレッドで呼ぶこと**。
    pub fn serve(self) {
        let picked = show_dialog();
        let _ = self.0.send(picked);
    }
}

#[cfg(not(windows))]
fn show_dialog() -> Option<PathBuf> {
    bring_to_front();
    rfd::FileDialog::new().pick_folder()
}

/// 窓をブラウザより前に出す。rfd は前に出さないので、呼ぶ側で手当てする (→ docs/folders.md「選び方」)。
///
/// Windows は、前面にいないプロセス (ブラウザで押した続きのサーバー) が窓を前面にするのを許さない。
/// 最前面に置く見えない窓を作って持ち主にし、窓をその上に出す。
#[cfg(windows)]
fn show_dialog() -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new();
    match windows_owner::TopmostOwner::new() {
        Some(owner) => dialog.set_parent(&owner).pick_folder(),
        None => dialog.pick_folder(),
    }
}

#[cfg(windows)]
mod windows_owner {
    use std::num::NonZeroIsize;

    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
        Win32WindowHandle, WindowHandle,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SetForegroundWindow, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    };
    use windows::core::w;

    /// 窓の持ち主にする、最前面の見えない窓。drop で消す。
    pub struct TopmostOwner(HWND);

    impl TopmostOwner {
        pub fn new() -> Option<Self> {
            // タスクバーに出さないよう WS_EX_TOOLWINDOW にする。
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                    w!("STATIC"),
                    None,
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .ok()?;
            // 許されれば、窓に文字を打てるよう前面にもする。許されなくても窓は最前面に出る。
            let _ = unsafe { SetForegroundWindow(hwnd) };
            Some(Self(hwnd))
        }
    }

    impl Drop for TopmostOwner {
        fn drop(&mut self) {
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    impl HasWindowHandle for TopmostOwner {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let hwnd = NonZeroIsize::new(self.0.0 as isize).ok_or(HandleError::Unavailable)?;
            let raw = RawWindowHandle::Win32(Win32WindowHandle::new(hwnd));
            // SAFETY: 窓は self と同じだけ生きる (drop で消す)。
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    impl HasDisplayHandle for TopmostOwner {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Ok(DisplayHandle::windows())
        }
    }
}

/// 窓をブラウザより前に出す。rfd は前に出さないので、呼ぶ側で手当てする (→ docs/folders.md「選び方」)。
///
/// macOS では、トレイだけのアプリ (やターミナルから動かした `weblav-service`) は前面のアプリにならないので、
/// 窓がブラウザの後ろに出る。窓を出す前に自分を前面にする。
#[cfg(target_os = "macos")]
fn bring_to_front() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // ターミナルから動かしたときは、前面に出られない扱い (Prohibited) になっている。
    if app.activationPolicy() == NSApplicationActivationPolicy::Prohibited {
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
    // macOS 14 からの `activate` は、ほかのアプリが譲ったときしか前面にならない。
    // ブラウザは譲らないので、非推奨でも強く前面にするほうを使う。
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}

#[cfg(not(any(target_os = "macos", windows)))]
fn bring_to_front() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_session_at_a_time() {
        let picker = FolderPicker::new(|| Ok(None));
        let session = picker.begin().expect("最初の番は取れるはず");
        assert!(picker.begin().is_none(), "開いている間は番を取れないはず");
        drop(session);
        assert!(picker.begin().is_some(), "閉じたら番を取れるはず");
    }

    #[test]
    fn requests_are_forwarded_to_the_receiving_thread() {
        let (picker, requests) = on_main_thread();
        let (tx, rx) = mpsc::channel();
        requests.forward(move |request| tx.send(request).is_ok());
        let handle = std::thread::spawn(move || picker.begin().expect("番を取れるはず").pick());
        let request = rx.recv().expect("頼みが届くはず");
        let _ = request.0.send(Some(PathBuf::from("/picked")));
        assert_eq!(
            handle.join().expect("スレッドが落ちた").ok().flatten(),
            Some(PathBuf::from("/picked"))
        );
    }

    #[test]
    fn a_picker_without_a_receiver_is_unavailable() {
        let (picker, requests) = on_main_thread();
        drop(requests);
        assert!(picker.begin().expect("番を取れるはず").pick().is_err());
    }
}
