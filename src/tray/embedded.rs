//! トレイ。サーバーを同じプロセスで動かし、「終了する」で一緒に止める。

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::TrayIconEvent;
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use weblav::folder_picker::{PickRequest, PickRequests};
use weblav::server::{Running, ServerExited};

use super::{
    POLL_INTERVAL, POLL_TIMEOUT, TRAY_TOOLTIP, TrayLocale, browser_origin, build_tray,
    detach_console, login_item, open_addr, setup, tray_locale,
};

enum UserEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
    // セットアップが要るか (管理者がまだいないか)。
    Setup(bool),
    // サーバーが自分で落ちた。
    ServerExited,
    // 「ログイン時に起動」の読み直しの結果。読めなければ `None`。
    LoginState(Option<login_item::State>),
    // フォルダー選択の窓を開く頼み (→ weblav::folder_picker)。
    PickFolder(PickRequest),
}

/// 起動済みのサーバーを受け取り、トレイを出す。
///
/// `tao` のイベントループ(`event_loop.run`)は正常終了時も `-> !` で戻ってこない。
/// そのため `server` の所有権をイベントループに移し、終了メニュー選択時に止める。
/// `pick_requests` は、サーバーからのフォルダー選択の窓を開く頼み。メインスレッドで開く。
pub fn run(server: Running, exited: ServerExited, pick_requests: PickRequests) {
    // タスクトレイ常駐モード。ターミナルから起動された場合でもそのターミナルは
    // 巻き込まず、このプロセスだけをコンソールから切り離す。
    detach_console();

    let addr = server.addr;
    let configured = server.configured;
    let mut server = Some(server);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Tray(event));
    }));

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Menu(event));
    }));

    // 管理者がまだいないかを定期的に確かめる。セットアップが済めばメニューから消し、
    // DB を作り直したときは出し直す。
    let poll_addr = open_addr(addr);
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        loop {
            let required = setup::required(poll_addr, POLL_TIMEOUT);
            // イベントループが終わっていれば送れない。そのときはこのスレッドも終える。
            if proxy.send_event(UserEvent::Setup(required)).is_err() {
                break;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });

    // サーバーが自分で落ちたら、トレイを閉じてプロセスを終える。
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        exited.wait();
        let _ = proxy.send_event(UserEvent::ServerExited);
    });

    let proxy = event_loop.create_proxy();
    pick_requests.forward(move |request| proxy.send_event(UserEvent::PickFolder(request)).is_ok());

    let locale = tray_locale();
    let setup_item = MenuItem::new(locale.setup_label(), true, None);
    let open_item = MenuItem::new(locale.open_label(), true, None);
    let manual_item = MenuItem::new(locale.manual_label(), true, None);
    let quit_item = MenuItem::new(locale.quit_label(), true, None);
    // 設定のポートが使えずにずらしたときだけ、メニューの先頭に出したままにする。
    // ほかの端末のブックマークが開けない理由に、気づけるように。
    let port_moved_item = (addr.port() != configured.port()).then(|| {
        MenuItem::new(
            locale.port_moved_label(configured.port(), addr.port()),
            false,
            None,
        )
    });
    // 「ログイン時に起動」。対応していない OS・版では出さない。
    // OS への問い合わせは応答を待つことがあるので、イベントループ (メインスレッド) を止めないよう
    // 別スレッドで行い、結果を `UserEvent::LoginState` で受け取る。その間は押せなくする。
    let mut login_state = login_item::supported().then_some(login_item::State::Off);
    let login_item = login_state
        .map(|state| CheckMenuItem::new(locale.login_item_label(state), false, false, None));
    let login_proxy = event_loop.create_proxy();
    let mut login_busy = login_item.is_some();
    if login_busy {
        spawn_login_task(login_proxy.clone(), None);
    }
    // 最初の問い合わせが返るまでは出さない。
    let mut setup_shown = false;

    // tray_icon はメインスレッドかつイベントループ稼働中に作る必要があるため、
    // NewEvents(Init) を待ってから生成する。
    // Drop されるとアイコンが消えるため、イベントループの外に出るまで保持し続ける。
    let mut _tray_icon = None;
    // 「セットアップ」を差し込む・取り除くために持っておく。
    let mut menu: Option<Menu> = None;

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                let root = Menu::new();
                let append = |item: &dyn IsMenuItem| {
                    root.append(item).expect("failed to append tray menu item");
                };
                if let Some(item) = &port_moved_item {
                    append(item);
                    append(&PredefinedMenuItem::separator());
                }
                // 問い合わせが Init より先に返っていることもある。
                if setup_shown {
                    append(&setup_item);
                }
                append(&open_item);
                append(&manual_item);
                append(&PredefinedMenuItem::separator());
                if let Some(item) = &login_item {
                    append(item);
                    append(&PredefinedMenuItem::separator());
                }
                append(&quit_item);

                _tray_icon = Some(build_tray(root.clone(), TRAY_TOOLTIP));
                menu = Some(root);
            }
            // OS の設定で変えられていることがあるので、メニューを開く前 (ポインタを載せた・押した) に読み直す。
            // 押したときの知らせはメニューが出た後に届くこともあるので、載せたときにも読む。
            Event::UserEvent(UserEvent::Tray(
                TrayIconEvent::Enter { .. } | TrayIconEvent::Click { .. },
            )) if login_item.is_some() && !login_busy => {
                login_busy = true;
                spawn_login_task(login_proxy.clone(), None);
            }
            Event::UserEvent(UserEvent::LoginState(read)) => {
                login_busy = false;
                if let (Some(item), Some(state)) = (&login_item, &mut login_state) {
                    if let Some(read) = read {
                        *state = read;
                    }
                    show_login_state(item, *state, locale);
                }
            }
            Event::UserEvent(UserEvent::Setup(required)) if required != setup_shown => {
                setup_shown = required;
                if let Some(menu) = &menu {
                    if required {
                        // 「セットアップ」は、ポートの知らせとその区切りの後ろ。
                        let position = if port_moved_item.is_some() { 2 } else { 0 };
                        let _ = menu.insert(&setup_item, position);
                    } else {
                        let _ = menu.remove(&setup_item);
                    }
                }
            }
            // Windows のサインアウト・シャットダウンのとき。
            // tao はこのイベントを送った直後にプロセスを終えるので、ここでサーバーを止めきる。
            Event::LoopDestroyed => {
                if let Some(server) = server.take() {
                    server.stop();
                }
            }
            // 窓を閉じるまでイベントループは止まる。窓はモーダルで、閉じるまでほかの操作を受けないため。
            Event::UserEvent(UserEvent::PickFolder(request)) => request.serve(),
            Event::UserEvent(UserEvent::ServerExited) => {
                tracing::error!("the server stopped unexpectedly");
                if let Some(server) = server.take() {
                    server.stop();
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                if event.id == quit_item.id() {
                    tracing::info!("quit menu item selected");
                    // `tao`は`ControlFlow::Exit`をセットすると事実上即座にプロセスを終了させる
                    // ため、先にサーバーを止めきっておかないと graceful shutdown が完走しない。
                    if let Some(server) = server.take() {
                        server.stop();
                    }
                    *control_flow = ControlFlow::Exit;
                } else if event.id == setup_item.id() {
                    // トークンの発行はサーバーへの問い合わせなので、イベントループ
                    // (メインスレッド) を止めないよう別スレッドで行う。
                    std::thread::spawn(move || {
                        let _ = open::that_detached(setup::url(poll_addr, POLL_TIMEOUT));
                    });
                } else if let Some(item) = login_item.as_ref().filter(|item| event.id == item.id())
                    && let Some(state) = login_state
                {
                    // muda は押された時点でチェックを反転させている。読み直しの途中なら、押す前に戻す。
                    if login_busy {
                        show_login_state(item, state, locale);
                    } else {
                        login_busy = true;
                        item.set_enabled(false);
                        let change = if state == login_item::State::NeedsApproval {
                            login_item::open_settings();
                            None
                        } else {
                            Some(item.is_checked())
                        };
                        // 失敗したときや許可が要るときも、読み直した実際の状態にチェックを合わせる。
                        spawn_login_task(login_proxy.clone(), change);
                    }
                } else if event.id == open_item.id() {
                    // `that` はランチャーの終了を待つことがあり、tao のイベントループ
                    // (メインスレッド)を止めうる (open 5.4.3 のドキュメント参照)。
                    let _ = open::that_detached(browser_origin(poll_addr));
                } else if event.id == manual_item.id() {
                    // マニュアルはログインしなくても読める (→ docs/help.md)。
                    let _ = open::that_detached(format!("{}/help", browser_origin(poll_addr)));
                }
            }
            _ => {}
        }
    });
}

/// 別スレッドで、`change` があれば OS に反映してから状態を読み直し、結果をイベントループへ送る。
fn spawn_login_task(proxy: EventLoopProxy<UserEvent>, change: Option<bool>) {
    std::thread::spawn(move || {
        if let Some(on) = change
            && let Err(err) = login_item::set(on)
        {
            tracing::warn!(error = %err, "failed to change the login item");
        }
        let read = login_item::state()
            .inspect_err(|err| tracing::warn!(error = %err, "failed to read the login item"))
            .ok();
        let _ = proxy.send_event(UserEvent::LoginState(read));
    });
}

/// 状態を、メニューの項目に反映する。
fn show_login_state(item: &CheckMenuItem, state: login_item::State, locale: TrayLocale) {
    item.set_text(locale.login_item_label(state));
    item.set_enabled(state.changeable());
    item.set_checked(state.checked());
}
