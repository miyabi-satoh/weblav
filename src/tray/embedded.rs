//! トレイ。サーバーを同じプロセスで動かし、「終了する」で一緒に止める。

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::TrayIconEvent;

use tray_icon::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use weblav::config::{self, TrayLanguage};
use weblav::folder_picker::{PickRequest, PickRequests};
use weblav::server::{Running, ServerExited};

use super::{
    ENGLISH_LABEL, JAPANESE_LABEL, POLL_INTERVAL, POLL_TIMEOUT, TRAY_TOOLTIP, TrayLocale,
    browser_origin, build_tray, detach_console, login_item, open_addr, resolve_locale, setup,
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
    #[cfg(windows)]
    login_item::register_restart_after_update();

    let addr = server.addr;
    let configured = server.configured;
    let open_setup_on_launch = server.open_setup_on_launch;
    let mut language = server.tray_language;
    let mut saver = Some(LanguageSaver::spawn(server.config_path.clone()));
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
        let mut open_setup_now = open_setup_on_launch;
        loop {
            let required = setup::required(poll_addr, POLL_TIMEOUT);
            // 入れた直後に起動しても窓は出ず、Windows 11 は新しいアイコンを「^」の中に隠すので、
            // 何も起きないように見える。起動したときに管理者がまだいなければ、セットアップの画面を開く。
            if open_setup_now && required {
                let _ = open::that_detached(setup::url(poll_addr, POLL_TIMEOUT));
            }
            open_setup_now = false;
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

    // macOS でアクセシビリティから押されたときの読み直し (`reread_before_press`) からも、今の言語を引く。
    let locale = Rc::new(Cell::new(resolve_locale(language)));
    let mut items = LabeledItems {
        setup: MenuItem::new("", true, None),
        open: MenuItem::new("", true, None),
        manual: MenuItem::new("", true, None),
        quit: MenuItem::new("", true, None),
        language: Submenu::new("", true),
        system_language: CheckMenuItem::new("", true, false, None),
    };
    let ja_item = CheckMenuItem::new(JAPANESE_LABEL, true, false, None);
    let en_item = CheckMenuItem::new(ENGLISH_LABEL, true, false, None);
    items
        .language
        .append_items(&[&items.system_language, &ja_item, &en_item])
        .expect("failed to append tray menu item");
    items.show(locale.get());
    show_language(
        language,
        language_choices(&items.system_language, &ja_item, &en_item),
    );
    // 設定のポートが使えずにずらしたときだけ、メニューの先頭に出したままにする。
    // ほかの端末のブックマークが開けない理由に、気づけるように。
    let port_moved_item = (addr.port() != configured.port()).then(|| {
        MenuItem::new(
            locale
                .get()
                .port_moved_label(configured.port(), addr.port()),
            false,
            None,
        )
    });
    // 「ログイン時に起動」。対応していない OS・版では出さない。
    // OS への問い合わせは応答を待つことがあるので、イベントループ (メインスレッド) を止めないよう
    // 別スレッドで行い、結果を `UserEvent::LoginState` で受け取る。その間は押せなくする。
    let mut login_state = login_item::supported().then_some(login_item::State::Off);
    let login_item = login_state
        .map(|state| CheckMenuItem::new(locale.get().login_item_label(state), false, false, None));
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
                    append(&items.setup);
                }
                append(&items.open);
                append(&items.manual);
                append(&PredefinedMenuItem::separator());
                append(&items.language);
                if let Some(item) = &login_item {
                    append(item);
                }
                append(&PredefinedMenuItem::separator());
                append(&items.quit);

                let tray = build_tray(root.clone(), TRAY_TOOLTIP);
                #[cfg(target_os = "macos")]
                super::press::open_menu_on_press(
                    &tray,
                    reread_before_press(login_item.clone(), locale.clone()),
                );
                _tray_icon = Some(tray);
                menu = Some(root);
            }
            // OS の設定で変えられていることがあるので、メニューを開く前 (ポインタを載せた・押した) に読み直す。
            // 押したときの知らせはメニューが出た後に届くこともあるので、載せたときにも読む。
            // アクセシビリティから押されたときは、`reread_before_press` が読む。
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
                    show_login_state(item, *state, locale.get());
                }
            }
            Event::UserEvent(UserEvent::Setup(required)) if required != setup_shown => {
                setup_shown = required;
                if let Some(menu) = &menu {
                    if required {
                        // 「セットアップ」は、ポートの知らせとその区切りの後ろ。
                        let position = if port_moved_item.is_some() { 2 } else { 0 };
                        let _ = menu.insert(&items.setup, position);
                    } else {
                        let _ = menu.remove(&items.setup);
                    }
                }
            }
            // Windows のサインアウト・シャットダウンのとき。
            // tao はこのイベントを送った直後にプロセスを終えるので、ここでサーバーを止めきる。
            Event::LoopDestroyed => shut_down(&mut saver, &mut server),
            // 窓を閉じるまでイベントループは止まる。窓はモーダルで、閉じるまでほかの操作を受けないため。
            Event::UserEvent(UserEvent::PickFolder(request)) => request.serve(),
            Event::UserEvent(UserEvent::ServerExited) => {
                tracing::error!("the server stopped unexpectedly");
                shut_down(&mut saver, &mut server);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                let choices = language_choices(&items.system_language, &ja_item, &en_item);
                if event.id == items.quit.id() {
                    tracing::info!("quit menu item selected");
                    // `tao`は`ControlFlow::Exit`をセットすると事実上即座にプロセスを終了させる
                    // ため、先にサーバーを止めきっておかないと graceful shutdown が完走しない。
                    shut_down(&mut saver, &mut server);
                    *control_flow = ControlFlow::Exit;
                } else if let Some(chosen) = choices
                    .into_iter()
                    .find_map(|(item, value)| (event.id == item.id()).then_some(value))
                {
                    // muda は押された時点でチェックを反転させているので、選んだものだけに付け直す。
                    show_language(chosen, choices);
                    if chosen != language {
                        language = chosen;
                        locale.set(resolve_locale(chosen));
                        items.show(locale.get());
                        #[cfg(target_os = "linux")]
                        if let Some(menu) = &menu
                            && let Some(replaced) = replace_submenu(
                                menu,
                                &items.language,
                                locale.get().language_label(),
                                &[&items.system_language, &ja_item, &en_item],
                            )
                        {
                            items.language = replaced;
                        }
                        if let Some(item) = &port_moved_item {
                            item.set_text(
                                locale
                                    .get()
                                    .port_moved_label(configured.port(), addr.port()),
                            );
                        }
                        if let Some(saver) = &saver {
                            saver.save(chosen);
                        }
                        // 「ログイン時に起動」は、持っている状態ですぐ今の言語にし、古いことがあるので読み直して合わせる。
                        // 読み直しや切り替えの途中なら、その結果が今の言語で出す。
                        if let (Some(item), Some(state)) = (&login_item, login_state) {
                            item.set_text(locale.get().login_item_label(state));
                        }
                        if login_item.is_some() && !login_busy {
                            login_busy = true;
                            spawn_login_task(login_proxy.clone(), None);
                        }
                    }
                } else if event.id == items.setup.id() {
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
                        show_login_state(item, state, locale.get());
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
                } else if event.id == items.open.id() {
                    // `that_detached` も、ランチャーが終わるのを最大 250ms 待つ (open 5.4.5)。
                    // tao のイベントループ (メインスレッド) を止めないよう、別スレッドで開く。
                    std::thread::spawn(move || {
                        let _ = open::that_detached(browser_origin(poll_addr));
                    });
                } else if event.id == items.manual.id() {
                    // マニュアルはログインしなくても読める (→ docs/help.md)。
                    std::thread::spawn(move || {
                        let _ = open::that_detached(format!("{}/help", browser_origin(poll_addr)));
                    });
                }
            }
            _ => {}
        }
    });
}

/// 表示言語で文言が変わる項目。選び直したときに、まとめて書き換える。
struct LabeledItems {
    setup: MenuItem,
    open: MenuItem,
    manual: MenuItem,
    quit: MenuItem,
    language: Submenu,
    system_language: CheckMenuItem,
}

impl LabeledItems {
    fn show(&self, locale: TrayLocale) {
        self.setup.set_text(locale.setup_label());
        self.open.set_text(locale.open_label());
        self.manual.set_text(locale.manual_label());
        self.quit.set_text(locale.quit_label());
        self.language.set_text(locale.language_label());
        self.system_language
            .set_text(locale.system_language_label());
    }
}

/// `old` のサブメニューを、文言を `text` にした同じ中身のものに差し替える。差し替えられなければ None (古いまま残る)。
///
/// FIX: muda の GTK 版は、サブメニューの文言を `set_text` で書き換えない (muda 0.21.2)。サブメニューの項目は、
/// アイコンを並べる箱の中にラベルを持ち、`set_text` は項目自身のラベルしか書き換えないため。
/// 作ってから付けると、その時の文言で出る。
#[cfg(target_os = "linux")]
fn replace_submenu(
    menu: &Menu,
    old: &Submenu,
    text: &str,
    children: &[&dyn IsMenuItem],
) -> Option<Submenu> {
    let position = menu.items().iter().position(|item| item.id() == old.id())?;
    let new = Submenu::with_items(text, true, children).ok()?;
    menu.remove(old).ok()?;
    menu.insert(&new, position).ok()?;
    Some(new)
}

/// 表示言語のメニューの項目と、それぞれで選ぶ言語。
fn language_choices<'a>(
    system: &'a CheckMenuItem,
    ja: &'a CheckMenuItem,
    en: &'a CheckMenuItem,
) -> [(&'a CheckMenuItem, TrayLanguage); 3] {
    [
        (system, TrayLanguage::System),
        (ja, TrayLanguage::Ja),
        (en, TrayLanguage::En),
    ]
}

/// 選んでいる表示言語にだけチェックを付ける。
fn show_language(language: TrayLanguage, choices: [(&CheckMenuItem, TrayLanguage); 3]) {
    for (item, value) in choices {
        item.set_checked(value == language);
    }
}

/// 終わる前に、選んだ表示言語を書き終え、サーバーを止めきる。選んだ直後に終わっても、選んだものを残すため。
fn shut_down(saver: &mut Option<LanguageSaver>, server: &mut Option<Running>) {
    if let Some(saver) = saver.take() {
        saver.finish();
    }
    if let Some(server) = server.take() {
        server.stop();
    }
}

/// 選んだ表示言語を `config.toml` に書くスレッド。ファイルの読み書きでイベントループ (メインスレッド) を
/// 止めないよう、別スレッドで書く。書けなくても、今動いている間のメニューは選んだ言語のままにする。
/// 1つのスレッドが届いた順に書き、溜まっていれば最後のものだけを書くので、ファイルは最後に選んだものになる。
struct LanguageSaver {
    sender: mpsc::Sender<TrayLanguage>,
    handle: std::thread::JoinHandle<()>,
}

impl LanguageSaver {
    fn spawn(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel::<TrayLanguage>();
        let handle = std::thread::spawn(move || {
            while let Ok(mut language) = receiver.recv() {
                while let Ok(next) = receiver.try_recv() {
                    language = next;
                }
                if let Err(err) = config::write_tray_language(&path, language) {
                    tracing::warn!(error = %err, "failed to save the tray language");
                }
            }
        });
        Self { sender, handle }
    }

    fn save(&self, language: TrayLanguage) {
        let _ = self.sender.send(language);
    }

    /// 頼んだ分を書き終えるまで待つ。
    fn finish(self) {
        drop(self.sender);
        let _ = self.handle.join();
    }
}

/// 別スレッドで、`change` があれば OS に反映してから状態を読み直し、結果をイベントループへ送る。
fn spawn_login_task(proxy: EventLoopProxy<UserEvent>, change: Option<bool>) {
    std::thread::spawn(move || {
        if let Some(on) = change
            && let Err(err) = login_item::set(on)
        {
            tracing::warn!(error = %err, "failed to change the login item");
        }
        let _ = proxy.send_event(UserEvent::LoginState(read_login_state()));
    });
}

/// アクセシビリティから押されたときに、読み終わるのを待ってメインスレッドを止めておく上限。
/// 間に合わないのは OS が詰まっているときだけで、そのときにメニューを長く待たせない。
#[cfg(target_os = "macos")]
const PRESS_READ_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// アクセシビリティから押されたときに、メニューを開く前に「ログイン時に起動」を OS から読んで見せる。
/// 押す操作はポインタを通らず、載せたときの読み直しが起きないため。
/// メニューを開いている間はイベントループが止まるので、`PRESS_READ_WAIT` まではここで待って項目に入れる。
/// 間に合わなければ前の状態のまま開き、結果は捨てる。メニューを開いている間に届いた結果で項目を変えると、
/// 利用者が押して変わったチェックを上書きするため。
/// 切り替えの途中 (押せない間) は、途中の状態を見せないよう読まない。
/// 変えるのは項目の見た目だけで、イベントループの `login_state` は次の読み直しで揃う。
#[cfg(target_os = "macos")]
fn reread_before_press(item: Option<CheckMenuItem>, locale: Rc<Cell<TrayLocale>>) -> Box<dyn Fn()> {
    Box::new(move || {
        let Some(item) = item.as_ref().filter(|item| item.is_enabled()) else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(read_login_state());
        });
        if let Ok(Some(state)) = receiver.recv_timeout(PRESS_READ_WAIT) {
            show_login_state(item, state, locale.get());
        }
    })
}

/// OS から「ログイン時に起動」を読む。読めなければ `None`。
fn read_login_state() -> Option<login_item::State> {
    login_item::state()
        .inspect_err(|err| tracing::warn!(error = %err, "failed to read the login item"))
        .ok()
}

/// 状態を、メニューの項目に反映する。
fn show_login_state(item: &CheckMenuItem, state: login_item::State, locale: TrayLocale) {
    item.set_text(locale.login_item_label(state));
    item.set_enabled(state.changeable());
    item.set_checked(state.checked());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 続けて選んでも最後に選んだものが残り、`finish` は書き終えるまで待つ。
    #[test]
    fn language_saver_writes_the_last_choice_before_finishing() {
        let dir = crate::test_support::TempDir::new("language-saver");
        let path = dir.join("config.toml");

        let saver = LanguageSaver::spawn(path.clone());
        saver.save(TrayLanguage::En);
        saver.save(TrayLanguage::System);
        saver.save(TrayLanguage::Ja);
        saver.finish();

        let text = std::fs::read_to_string(&path).expect("書かれているはず");
        let config: weblav::config::Config = toml::from_str(&text).expect("設定を読めるはず");
        assert_eq!(config.tray.language, TrayLanguage::Ja);
    }
}
