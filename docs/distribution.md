# 配布 (ビルド・常駐・MSIX)

Windows 版の配布物を作る手段。
ランタイムや DB サーバーを別に入れさせず、WebLAV の実行ファイルだけで動くようにする。

## ビルド・配布の方法

- **配る相手は Windows・macOS・Ubuntu (デスクトップ)**。どれも、ログインした人のトレイに常駐する同じ形で動かす。
  - **機能と操作は OS で変えない**。マニュアル (→ help.md) のほとんどが、どの OS でも同じ手順で通じるようにするため。
    OS で違ってよいのは、入手・インストール・置き場所・OS の設定画面のように、OS の都合で決まるところだけ。
  - 画面の無い Linux (systemd で `weblav-service` を動かす形) は、「共有フォルダを設定できない人」に向けた WebLAV の相手から外れるので、対象にしない。
  - Ubuntu のトレイは、Windows・macOS と同じ `tao`・`tray-icon` で出す。アイコンのクリックは届かず右クリックのメニューだけになるので、トレイの操作はメニューに置く。
    起動のたびにログへ出る `gtk_widget_get_scale_factor: assertion 'GTK_IS_WIDGET (widget)' failed` は、`tray-icon` が使う libayatana-appindicator 由来で、トレイの動きには響かない (tao #534)。
  - **macOS は、App Store からも配れるよう、サンドボックスで動く作りにする** (App Review Guidelines 2.4.5 (i))。App Store から配っても、開発者 ID の署名と公証で自分のサイトから配っても、作り直さずに済むようにする。
    - 権限 (`installer/macos/weblav.entitlements`) は、サンドボックス・ネットワークの受信と送信・窓で選んだ場所を読むこと・ブックマークだけ。
      ad-hoc 署名のままでも、この権限でサンドボックスの中で動く (実機で確認)。`just bundle-mac` の .app は常にサンドボックスで動かす。
    - 公開できるフォルダは、OS のフォルダ選択の窓 (NSOpenPanel) で選ばせる (→ folders.md「公開できるフォルダ」)。サンドボックスでは、利用者が窓で選んだ場所しか読めず、その許可は起動し直すと消える。
      窓で選んだときに security-scoped bookmark (読むだけ) を作って DB (`folder_bookmarks`) に残し、起動のたびとバックアップから戻したときに、登録中のフォルダの分だけ許可を戻す (`src/folder_access.rs`)。ブックマークを作れなければ、選んだ時点でエラーにして登録へ進ませない。
      ブックマークはデータではなくこの Mac の状態なので、バックアップから戻しても置き換えず、今の行を残す。
      戻せないもの (フォルダが消えた・この Mac で選んだことが無い) は、存在しないフォルダと同じく読めないまま。戻せなくても、起動やバックアップからの復元は止めない。
    - 設定とデータの置き場所は、サンドボックスのコンテナ (`~/Library/Containers/com.amiiby.weblav/Data/Library/Application Support/com.amiiby.weblav`)。
    - まだ配っていないので、サンドボックスより前の版からの移行 (データをコンテナへ移す・登録済みのフォルダの許可を取り直す) は作らない。
    - ブラウザを開く (`open` crate) と「ログイン時に起動」(`SMAppService`) は、サンドボックスの中でもそのまま使える。
  マニュアル (→ help.md) には各 OS の手順を書く。
- Windows 版のビルドは **Windows の PC で手動**で行う (CI は Linux で `just ci` を流すだけで、配布物は作らない。→ DEVELOPMENT.md)。
- **Windows は Microsoft Store の MSIX だけで配る** (→ 「MSIX (Windows)」)。exe だけやほかのインストーラでは配らない。マニュアル (→ help.md) は Store 版で書く。
  - `just msix` で **`dist/weblav-v<version>.msix`** を作る。どの版か区別できるように、バージョン番号をファイル名に入れる。
  - Store に出すまで試すときは、自己署名の MSIX を入れる (→ 「MSIX (Windows)」)。
- 中の exe (`weblav.exe`) のビルドは `just build` と同じで、バージョン情報・アイコンの埋め込みは `build.rs` (→ 「バージョン・アイコンの埋め込み」) が毎回行うため専用の手順は要らない。
- **MSVC のランタイムは静的リンクする** (`.cargo/config.toml` の `+crt-static`)。
  動的リンクでは `vcruntime140.dll` などが要り、Visual C++ の再頒布可能パッケージが無い素の Windows で起動できないおそれがある。
  ビルドする PC には Visual Studio が入っているので、動的リンクのままでは確かめても気づけない。
  再頒布可能パッケージを同梱するより、配布物も手順も増えない。
- macOS は `just bundle-mac` で `target/release/bundle/WebLAV.app` を作る (`scripts/bundle-mac.mjs`、`installer/macos/Info.plist`)。
  証明書なしの署名 (ad-hoc) だけで、公証はしていない。
  `just install-mac` は、それを `/Applications` に入れ直して起動し直す。

## バージョン・アイコンの埋め込み

- `winresource` (build-dependency、`cfg(windows)` 限定) が `build.rs` で
  exe に製品名・バージョン・アイコンを埋め込む。`[package.metadata.winresource]`
  (`Cargo.toml`) で `ProductName`・`FileDescription`・`OriginalFilename` を指定する
  (`FileDescription` は既定だと crate 名 `weblav` になり、Windows がツールやダイアログに
  アプリ名として出す値のため明示的に上書きする)。バージョンは `[package] version` を
  そのまま使う。
  リソースはパッケージ内の全 exe にリンクされるため、`weblav-cli.exe`・`weblav-service.exe` も同じアイコン・バージョン情報を持つ
  (`OriginalFilename` も `weblav.exe` のまま。exe ごとに分ける手段が無く、実害も無いため許容する)。
- アイコンの絵 (`assets/icon.svg`、差し色 #146E6B と白の「W」) から
  `assets/icon.ico` (16/20/24/32/48/256px、exe 埋め込み用) と
  `assets/tray-icon-64.rgba` (タスクトレイ用、生の RGBA) を
  `just generate-icon` (`scripts/generate-icon.mjs`、sharp + to-ico、Node製) で作る。
  ブラウザのタブのアイコン (`frontend/src/lib/assets/favicon.svg`) も、同じ手順で `icon.svg` を写して作る。
  どちらも生成物として **コミットする** (`build.rs` はビルドのたびに `.ico` を要求するため)。
  Node製にしたのは Mac でも同じ手順で作り直せるようにするため
  (Inkscape 等の Windows 専用ツールを要らないようにする)。
- タスクトレイのアイコンは実行時に `include_bytes!` で埋め込む
  (画像デコード用の crate を増やさないため、生の RGBA のまま持つ)。
  実機で 100%・150% 表示スケーリングの両方で確認済み。
- macOS のメニューバーでは、`assets/tray-icon-mac.svg` (角丸の枠線と「W」の単色の線画) から作る
  `assets/tray-icon-mac-64.rgba` を、テンプレート画像として出す。
  メニューバーのアイコンは単色で、明暗は OS が付けるのが macOS の作法のため。

## 版の番号

- 版は `Cargo.toml` の `[package] version` (`x.y.z`)。**Store に出すたびに上げる**。MSIX は版番号が上がっていないと上から入らず、前の版を使っている人に届かないため。
  直しだけなら z、機能を足したら y を上げる。
- ビルド番号は、ビルドしたコミットまでのコミットの数 (`git rev-list --count HEAD`)。`build.rs` が数えて実行ファイルに入れ (`WEBLAV_BUILD`)、サイト設定の「このアプリについて」に版の後ろに括弧で添えて出す (`1.0.0 (123)`)。
  - 版を上げずに作り直したものも見分けられ、問い合わせで読み上げてもらえばどのコミットから作ったかが分かる。
  - 試しの MSIX の版番号の4つ目と同じ数え方 (→「MSIX (Windows)」)。git の無い場所でビルドしたときは版だけを出す。

## コンソールの扱い

- **ダブルクリック起動 (タスクトレイ常駐) では、コンソールを一切出さない**。
  `main.rs` に `windows_subsystem = "windows"` を設定し、コンソールの生成自体を無くす
  (リリースビルドのみ。`debug_assertions` の間は `cargo run`/`just dev-backend` でコンソール付き)。
  コンソール付きで起動して `FreeConsole` で切り離す形は採らない。起動処理 (DB接続・ポート bind) の間は
  コンソールが実在し、その間にユーザーが誤って閉じると、`CTRL_CLOSE_EVENT` で切り離し前にプロセスごと終了するため。
- **CLI (`--create-user`・`--openapi`・`--version`) は、コンソールサブシステムの別 exe `weblav-cli` に分ける**
  (`src/bin/weblav-cli.rs`、中身は `weblav::cli`)。開発・テスト用で、リリース版には入れない (→ access.md「初回セットアップ」)。
  GUI サブシステムの exe は、コマンドプロンプトから CLI として使えないため (実機で確認)。
  - コマンドプロンプトは GUI サブシステムの子プロセスの終了を待たずにプロンプトへ戻る。
    `--create-user` のパスワード入力が cmd 自身の入力と取り合いになり、打ったパスワードがコマンドとして解釈される。
  - リダイレクト先のハンドルを子プロセスに渡さない (`STARTF_USESTDHANDLES` を使わない)。
    `>` で書き出したファイルは空になる。
  - `AttachConsole` で親のコンソールに繋ぎ直しても、上の2つはプロセスの内側からは解消できない。
- **サーバーだけを動かす `weblav-service` もコンソールサブシステム**にする。
  ターミナルから起動して、そのまま出力を見ながら確かめられるようにするため (→ 「常駐 (Windows)」)。
- `weblav` 本体は引数付きで呼ばれたら、常駐せずに引数を受け付けない旨を伝えて終える (メッセージボックス、→ 次項)。
- **タスクトレイを出す前の起動失敗**は、stderr に加えて**メッセージボックスでも伝える** (`fatal_startup_error`)。
  ディレクトリの解決・設定の読み込み・ログの初期化・DB 接続・ポートの bind などが該当する (ログの初期化の後はログにも残る)。
  二重起動は失敗として扱わず、動いているほうの画面を開く (→ 「常駐 (Windows)」)。
  ログの初期化までの失敗 (引数・ディレクトリの解決・設定の読み込み・ログの初期化) は、まだログが無いのでログには残らない。
  ダブルクリック起動ではコンソールが無く、何も出さずに終了するとユーザーには起動しなかった
  ことしか分からないため。
- ログの出力先は既定で `file` (`LogOutput::File`、→ `config.example.toml`) であり、
  コンソールの有無に関わらず動く。`output = "stdout"` を明示した場合、常駐モードでは
  コンソールが無いため出力先が無く消える (ユーザー判断: 既定のままとし、
  `config.example.toml` に注記するだけにする。`stdout` は開発時にターミナルから
  動かす用途)。

## 初回起動時の警告

- Store から入れた MSIX には Microsoft の署名が付くので、SmartScreen の警告は出ない見込み (→ 「MSIX (Windows)」)。Store に出した後でないと確かめられない。
- ファイアウォールの規則はマニフェストの宣言で入るので、サーバーが起動しても「Windows セキュリティ」のダイアログは出ない (自己署名の MSIX で確認、→ 「MSIX (Windows)」)。

## 常駐 (Windows)

Windows では、Store の MSIX (→ 「MSIX (Windows)」) で入れ、ログインした人のタスクトレイに常駐するアプリとして動かす。
サーバーはトレイと同じプロセスで動く。macOS と同じ形。

### サービスにしない理由

- Microsoft Store から MSIX で配ると、署名を Microsoft に任せられる (→ 「MSIX (Windows)」)。
  サービスを MSIX に入れるには `packagedServices` の宣言が要り、Microsoft は「Store に出すアプリでは、ほとんどの場合承認しない」と書いている ([App capability declarations](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations))。
- ログインしたときの自動起動で、使い勝手はほぼ変わらない。
  誰もログインしていないときに使えないことと、1台を複数のアカウントで同時に使う場合は、対象外にする。1台の PC につき、サーバーは1つだけ動かす想定。
- 同種のサーバーにも前例がある。Plex Media Server は、Windows ではログインしたときに起動し、トレイから操作するアプリ。

### exe の分担

| exe | 役割 | 配布 |
|---|---|---|
| `weblav.exe` | タスクトレイとサーバー。ウィンドウを持たない | する |
| `weblav-cli.exe` | 開発・テスト用 (→ 「コンソールの扱い」) | しない |
| `weblav-service.exe` | サーバーだけ。開発 (`just dev-backend`・e2e) で使う | しない |

- サーバーの起動と停止は `weblav::server` (`start` と `Running::stop`) にまとめ、トレイと `weblav-service` が同じものを呼ぶ。

### タスクトレイ (`weblav`)

- メニューは「セットアップ」「ブラウザで開く」「マニュアル」「ログイン時に起動」「終了する」。
- **「ログイン時に起動」はチェック付きの項目で、OS の実際の状態を映す** (`src/tray/login_item.rs`)。
  オン・オフは weblav の設定に持たず、アイコンにポインタを載せたときと押したときに読み直す。利用者は OS の設定からも切り替えられるため。
  muda にはメニューが開く直前の知らせが無いので、その手前のトレイのイベントで読む。
  - Windows: マニフェストの `StartupTask` を切り替える。パッケージの中からスタートアップ フォルダに書いても、パッケージごとの場所に回されて効かないため (→ 「MSIX (Windows)」)。
    `StartupTask` はパッケージの中でしか使えないので、パッケージの外 (開発中の exe) では項目を出さない。
    項目名はマニュアルに合わせて「サインイン時に起動」と書く。
    利用者が「設定」やタスクマネージャーで止めたもの (`DisabledByUser`)、ポリシーで決まっているものはアプリから変えられないので、押せなくして理由を添える。
  - macOS: `SMAppService` で .app をログイン項目に登録する。macOS 13 より前 (Info.plist の下限は 11) は項目を出さない。
    システム設定での許可待ち (`RequiresApproval`) のときは、押すとシステム設定のログイン項目を開く。
  - Linux: XDG Autostart の `~/.config/autostart/weblav.desktop` を作る・消す (`XDG_CONFIG_HOME` があればその下)。
    デスクトップの設定で止めた印 (`Hidden=true`・GNOME の `X-GNOME-Autostart-enabled=false`) があればオフとして出す。作り直せば有効に戻るので、押せなくはしない。
    トレイのイベントが届かないので、読み直すのは起動したときと押したときだけ。
- **設定のポートが使えなければ、上の番号へ最大10個ずらして起動する** (`OnPortUnavailable::TryNext`)。使えないとみなすのは、使用中と、Windows で Hyper-V などが予約した範囲 (アクセス拒否) のとき。
  - ずらした番号は `config.toml` に書かず、起動のたびに設定のポートから試す。ずれるのは別のソフトがポートを取ったような事故と見て、原因を取り除けば元に戻るようにするため。いつもずれるなら、管理画面のサイト設定でポートを変えてもらう (→ ui.md「UI 全般」)。
  - ずれている間は、メニューの先頭に押せない項目で「ポート 3000 を使えないため、3001 で動いています」と出し続ける。ほかの端末のブックマークが開けない理由に気づけるように。tray-icon には通知 (バルーン) を出す口が無いため、メニューに出す。
  - mDNS と「他の端末からつなぐ」は、実際のポートを案内する。
  - `weblav-service` はずらさずに失敗する (`Fail`)。開発や e2e で動かすとき、黙って別のポートで動かないように。
- **「セットアップ」は、`admin` がまだ1人もいないときだけ出す** (→ access.md「初回セットアップ」)。3秒ごとに `/api/v1/setup/status` へ問い合わせて出し入れする。
  HTTP クライアントの crate は足さず、std の TCP で1往復する。
- 「終了する」でサーバーも止める。処理中の応答は最大10秒だけ待つ (`SHUTDOWN_GRACE`)。動画の再生などで接続が続くと、いつまでも止まらないため。
- サーバーが自分で落ちたら (`ServerExited`)、トレイも閉じてプロセスを終える。トレイだけ残しても意味がないため。
- Windows のサインアウト・シャットダウン (`WM_ENDSESSION`) でも、サーバーを止めてから終える。
  tao はこのとき `LoopDestroyed` を送った直後にプロセスを終えるので、そこで止める。
- **二重起動のロック (`single_instance`) で1つに絞る**。ロックファイルはデータの置き場の `weblav.lock`。
  重ねて起動したほうは、エラーを出す代わりに、動いているほうの画面をブラウザで開いて終える。管理者がまだいなければ、セットアップの画面を開く。
  開くアドレスは、動いているほうがデータの置き場の `weblav.addr` に書いた実際の待ち受けのアドレス。ポートをずらして起動していることがあるため。
  - 動いているほうは、応答できるようになってから書く。重ねて起動したほうは、これが現れるまで最大15秒待ってから開く先を決め、現れなければ `config.toml` のアドレスを使う。
    待ち受け自体は DB の準備より前に始まる (ずらした番号を先に決めるため) ので、つながるかでは起動の途中かを見分けられない。
  - ロックファイルに書かないのは、Windows のファイルロックがほかのプロセスからの読み取りも止めるため。
  スタートアップで起動したあとにスタートメニューから開いたとき、ウィンドウが無いので、何も起きないと起動しなかったように見えるため。
- 起動の失敗はメッセージボックスで伝える (→ 「コンソールの扱い」)。

### 設定とデータの置き場所

- ログインした人ごとの置き場所に置く (`directories::ProjectDirs`)。macOS・Linux と同じ決め方。
  - 設定: `%APPDATA%\amiiby\weblav\config`
  - データ: `%LOCALAPPDATA%\amiiby\weblav\data`
  - MSIX で入れると、ここに書いたものはパッケージごとの場所に回される (→ 「MSIX (Windows)」)。
- 環境変数 `WEBLAV_HOME` があれば、そちらを使う (`just dev-backend` など)。
- 置き場の中は、公開できるフォルダの中にあっても一覧・スキャン・配信から外す (→ folders.md「公開できるフォルダ」)。設置者が親フォルダを選んでも巻き込まれない。

### 読めるファイル

- サーバーはログインした人として動くので、その人が読めるファイルだけを配れる。
- 登録したフォルダの外を読ませない守り (→ folders.md「公開できるフォルダ」) は、これまでどおり。

### 実機での確認

Windows の実機で次を確かめた。
- スタートメニューからもう一度開いても、トレイのアイコンは増えず、ブラウザでセットアップ画面が開く。
- 管理者は、セットアップ画面から作れる。
- 「終了する」で、プロセスも待ち受けのポートも消える。

フォルダの登録・UNC パス・初回セットアップのループバックの制限は、Windows のサービスとして `weblav-service` を動かしていた版で確かめた。サーバーの作りが同じなので、常駐アプリでも当てはまる見込み。

### ほかの OS

- macOS は Windows と同じく、トレイがサーバーを同じプロセスで動かす (`tray::run`)。
  ログイン項目でメニューバーに常駐させる。登録はトレイの「ログイン時に起動」で行う (→ タスクトレイ)。サービス (LaunchDaemon) にすると、プライバシー保護 (TCC) のため、書類や外付け・ネットワークのボリュームを読むのに事前にフルディスクアクセスの許可が要る。
- Ubuntu も同じく、トレイがサーバーを同じプロセスで動かす (`tray::run`)。ログイン時の起動は XDG Autostart で行う (→ タスクトレイ)。

## MSIX (Windows)

Windows は Microsoft Store から MSIX で配り、ほかの形では配らない。
Store に MSIX で出すと、審査のあとで Microsoft が署名し直すので、証明書を買わずに SmartScreen の警告を避けられる ([Code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options))。
Smart App Control も、Store の署名がある MSIX を許可する範囲に入れている ([Smart App Control の許可の規則](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/design/create-appcontrol-policy-for-lightly-managed-devices))。

- マニフェストは `installer/msix/AppxManifest.xml`。`just msix` (`scripts/msix.mjs`) がバージョンと発行元を埋め、exe とロゴ (`assets/msix/`、`just generate-icon` で作る) を並べて `makeappx` で固め、試しに入れる版は `signtool` で署名する (`--store` では署名しない)。要るのは Windows SDK。
- 中身は `weblav.exe` (→ 「exe の分担」)。
- 宣言するもの
  - `runFullTrust`: 普通のデスクトップアプリとして動かす。Store で「ほとんど承認しない」とされる機能 (`packagedServices` など) は使わない
  - `privateNetworkClientServer`: LAN から受ける。`internetClientServer` は付けない
    (Windows の実機で確かめたところ、付けると Windows が自動で足す受信規則にパブリックのネットワークまで含まれると分かった。LAN の中だけで使う前提に反する)
  - `windows.startupTask`: ログインしたときに起動する。利用者が一度起動した後に有効になり、「設定」やタスクマネージャーで止められる
  - `windows.firewallRules`: `weblav.exe` への TCP の受信を、プライベートとドメインのネットワークだけで許可する
- **Windows 11 は新しく接続したネットワークを既定でパブリックにする**。
  家庭の Wi-Fi でもパブリックのままだと他の端末からつながらないので、マニュアルでプライベートへの切り替えを案内する。
  トレイでパブリックであることを知らせる機能は持たない。
  有線・Wi-Fi・VPN が重なるとどのネットワークを見るか決めにくく、出先ではパブリックが正しい状態で、確かめる場所も直す場所も Windows の設定のため。
- 試しに入れるときの署名は、`installer/msix/new-test-cert.ps1` で作る自己署名の証明書 (`CN=WebLAV Test`) で行う。
  このスクリプトは、証明書を作る PC の「信頼されたユーザー」(LocalMachine の TrustedPeople) にも入れる。別の PC に入れるときは、そちらでも同じストアに入れる。
- Store に上げる版は `just msix --store` で作る (`dist/weblav-v<version>-store.msix`)。発行元をパートナー センターが示す値にし、署名はしない (Store が署名する)。
  - `Name`: `amiiby.WebLAV`・発行元の表示名: `amiiby`
  - Package Family Name: `amiiby.WebLAV_tv82n7df3ay6j`。マニュアルの「運用する」の置き場所はこれで書いている。自己署名で試しに入れたものは発行元が違うので、名前の後ろが変わる。
- 最初の管理者は、入れるときには作らず、トレイの「セットアップ」から作る (→ access.md「初回セットアップ」)。
- **Store に出す版番号は、先頭を 1 以上にする**。`Cargo.toml` の `x.y.z` から `x.y.z.0` を作る。
  4つ目は Store が使うので 0 のまま、先頭は 0 にできない ([App package requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements))。WACK はこれを咎めない (`0.1.0` でも通った)。
- 同じ版番号のまま中身を替えたものは、上から入れられない (0x80073CFB。`-ForceUpdateFromAnyVersion` でも同じ)。
  外すとデータ (下記) とログイン時の起動の状態も消えるので、試しの版は版番号の4つ目をコミットの数にして、コミットが進むたびに上から入れられるようにしている。
  `just install-windows` が、作って上から入れ直し、起動し直す。

### 設定とデータの置き場所

- `%APPDATA%`・`%LOCALAPPDATA%` に書いたものは、パッケージごとの場所 (`%LOCALAPPDATA%\Packages\<パッケージ名>\LocalCache\{Roaming,Local}`) に回される (実機で確認)。
  - アンインストールで、設定とデータ (DB・アップロードしたファイル) は尋ねられずに消える。
- 消えるのは Store アプリの普通の動きなので、置き場所は変えずに受け入れる。アンインストールの前にバックアップ (→ access.md「バックアップとリストア」) が要ることは、マニュアルの「運用する」で伝える。
  - 仮想化を外す `unvirtualizedResources` は、資料で PC ゲーム向けとされ、ほかの用途は想定されていない ([App capability declarations](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations))。Store の審査を通らない見込みなので使わない。
  - `AppData` の外に置けば回されないが、Windows の普通の置き場所から外れ、アンインストールしても残り続けるので採らない。

### 実機での確認

自己署名で作ったものを Windows の実機に入れて確かめた。

- ファイアウォールの規則は、ダイアログなしで入る。対象はプライベートとドメインのネットワークだけで、LAN の他の端末からつながる。
- 起動するとトレイに出る。スタートメニューからもう一度開くと、ブラウザで画面が開く。ログインで自動起動するように登録される (実際のサインインでは確かめていない)。
- Smart App Control がオンの PC で動くかは、確かめていない。Store の署名が付くのは Store から入れたときだけなので、Store に出した後でないと確かめられない。
- WACK (Windows App Certificate Kit) の必須のテストはすべて通る (WACK 10.0.26100.8249)。リポジトリの直下で、管理者の PowerShell から流す (`/?` にも管理者の権限が要る)。
  ```
  $appcert = "${env:ProgramFiles(x86)}\Windows Kits\10\App Certification Kit\appcert.exe"
  & $appcert reset
  & $appcert test -appxpackagepath "$PWD\dist\weblav-v<version>.msix" -reportoutputpath "$PWD\data\wack-report.xml"
  ```
  - `appcert.exe` はフルパスで呼ぶ。管理者で開き直した PowerShell では PATH に入っていない。
  - `-appxpackagepath`・`-reportoutputpath` は絶対パスで渡す。相対パスでは「指定されたパッケージ ファイルは存在しません」「有効なパスである必要があります」で止まる。
  - 落ちるのは任意のテスト「ブロック済みの実行可能ファイル」だけ。`weblav.exe` が `CreateProcessW` と `PowerShell`・`cmd.exe` などを参照しているため (`PowerShell` はトレイから URL を開く `open` クレート)。
  - 任意のテストは Store の審査の判定に使われず ([Windows Desktop Bridge app tests](https://learn.microsoft.com/en-us/windows/uwp/debug-test-perf/windows-desktop-bridge-app-tests))、このテストは S モードの Windows で動かないおそれを知らせるものなので、直さない。
