//! macOS で、メニューバーのアイコンを VoiceOver などのアクセシビリティから「押した」ときにメニューを開く。
//!
//! tray-icon (0.25 以降) は、アイコンにメニューを付けるのをマウスで押している間だけにしている
//! (メニューを付けたままだと、クリックのイベントが届かないため。tray-icon#365)。
//! アクセシビリティの押す操作 (AXPress) はマウスのイベントを通らず、ボタンの action を送るだけなので、
//! メニューの付いていないアイコンでは何も起きない。VoiceOver で状況メニューを開く操作 (VO-M-M → VO-Space) は
//! macOS の標準なので、ボタンの action でメニューを開く。

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSApplication;
use tray_icon::TrayIcon;

struct Ivars {
    tray: TrayIcon,
    // メニューを開く処理 (`show_menu`) がもう一度ボタンを押すので、その間に届いた action では開き直さない。
    pressing: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WeblavTrayPressTarget"]
    #[ivars = Ivars]
    struct PressTarget;

    impl PressTarget {
        #[unsafe(method(press:))]
        fn press(&self, _sender: Option<&AnyObject>) {
            let ivars = self.ivars();
            if ivars.pressing.replace(true) {
                return;
            }
            show_menu(&ivars.tray, self.mtm());
            ivars.pressing.set(false);
        }
    }
);

/// アイコンのボタンが action を送ったときに、メニューを開くようにする。メインスレッドで呼ぶ。
pub fn open_menu_on_press(tray: &TrayIcon) {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::error!("tray press: not on the main thread");
        return;
    };
    let Some(button) = tray.ns_status_item().and_then(|item| item.button(mtm)) else {
        tracing::error!("tray press: the status item has no button");
        return;
    };
    let target = PressTarget::alloc(mtm).set_ivars(Ivars {
        tray: tray.clone(),
        pressing: Cell::new(false),
    });
    // SAFETY: NSObject の init は引数を取らず、初期化した自分を返す。
    let target: Retained<PressTarget> = unsafe { msg_send![super(target), init] };
    // SAFETY: target は press: を持ち、下の forget でアプリが動いている間生きている。
    unsafe {
        button.setTarget(Some(&target));
        button.setAction(Some(sel!(press:)));
    }
    // ボタンは target を弱く持つだけなので、手放さずに持ち続ける。アイコンはアプリが動いている間ずっと出ている。
    std::mem::forget(target);
}

fn show_menu(tray: &TrayIcon, mtm: MainThreadMarker) {
    // 前面にいないまま押されると、macOS がメニューを開きながらアプリを前面に出し、その知らせでメニューが閉じる。
    // 先に前面にしておく (WebLAV は窓を持たず、起動しても前面にならない)。
    let app = NSApplication::sharedApplication(mtm);
    if !app.isActive() {
        // 代わりの activate は macOS 14 からで、動作環境の下限 (Info.plist の 11) に無い。
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
    // メニューを閉じるまで戻らない。
    tray.show_menu();
}
