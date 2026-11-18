# OS ごとの自動の確認

`just ci` の単体テスト・API のテスト・Playwright の E2E (`frontend/e2e/`) では通らない、OS とつながる所を、配る形のアプリで動かして見る。CI の runner には OS のメニューバーやデスクトップが無いので、その OS の機で回す。

## macOS の確認

```
just macos-check          # macos/*.test.mjs を全部
just macos-check tray     # macos/tray.test.mjs だけ
```

見るもの: メニューバーのメニュー (`tray`)、多重起動 (`launch`)、フォルダー選択の窓・選んだフォルダーのブックマーク・ごみ箱 (消す・上書き)・別のボリュームへの移動 (`folders`)、NAS の共有 (`nas`)、mDNS の名前 (`mdns`)、ログイン時に起動とシステム設定の一覧から外したとき (`login-item`)。

- **ビルドしない**。`/Applications/WebLAV.app` を動かす (ほかのものは環境変数 `WEBLAV_APP` で指す)。変更を見るときは、先に `just install-mac` で入れ直す。
- **常用の WebLAV とは別に動かす**。データの置き場 (`WEBLAV_HOME`) をコンテナの中の `weblav-e2e/` に、ポートを 18370 にして並べて動かし、常用のものは止めない。終わったらデータの置き場を消す。
- **ログインした画面のセッションの端末で、画面のロックを解いて流す**。回している間は `caffeinate` で画面を眠らせない。launchd から直に立った処理の下 (Herdr のタブなど) では、アクセシビリティの許可が効かず、始める前に止まる。
- **許可**: 回す端末に、アクセシビリティの許可が要る。ごみ箱を Finder から読むので、初めは「Finder を操作する」許可の確認が出る (許可する)。
- **キーボードとフォーカスを使う** (フォルダー選択の窓に場所を打つ)。回している間はキーボードとマウスに触らない。打つたびに WebLAV が前面かを確かめ、外れていたら打たずに落ちる。打つ間だけ英字の入力ソースにし、落ちても戻す (日本語のままだと変換に入るため)。
- **メニューは開いたままにしない**。項目はアクセシビリティから読んで押す (`lib/macos.mjs` の `readMenu`)。メニューバーのアイコンが多いと、確かめ用のアイコンが画面に入りきらず、開いても出ないため。
- **ログイン時に起動**は、登録がアプリの束ごとなので、常用の WebLAV の登録を切り替えて元に戻す。`/Applications/WebLAV.app` 以外の束では回さない (戻したときに、登録がその束を指してしまう)。システム設定の「ログイン項目」を開いて一覧から外し、閉じる。
- **NAS の共有** (`nas`) は、環境変数 `WEBLAV_E2E_SMB` に手元の SMB の共有 (`smb://<ユーザー>@<ホスト>/<共有>`) を書いたときだけ回す。共有の直下に確かめ用のフォルダー (`weblav-e2e-<ランダムな6文字>`) を作って消し、途中で共有を一度取り出す (ほかで使っている最中には回さない)。パスワードはキーチェーンから読む。回す前に載っていなければ、終わったら取り出す。
- **残るもの**: ごみ箱に、消したものと上書きしたもの (`weblav-e2e-<時刻>.txt`・`weblav-e2e-overwritten-<時刻>.txt`) が入る。多重起動の確認では、2つ目の起動が既定のブラウザーで確かめ用の WebLAV の画面を開き、そのページが残る。

## MSIX 版の確認

```
just msix-check              # msix/*.test.mjs を全部
just msix-check tray         # msix/tray.test.mjs だけ
just msix-check --no-build   # 作り直さずに、前に作った MSIX で回す
```

見るもの: パッケージの更新・データの置き場・ファイアウォールの規則・アンインストール (`package`)、トレイのメニューとサインイン時に起動・多重起動 (`tray`)、フォルダー選択の窓・ごみ箱・使用中のファイル (`folders`)、NAS の共有と NAS が戻ったとき (`nas`)、電源の切れた NAS (`offline`)、mDNS の名前 (`mdns`)、WACK (`wack`)。

- **作るもの**: `just msix` で配る形の MSIX (`dist/weblav-v<版>.msix`。WACK にかける) を作り、同じ exe から確かめ用のパッケージ (`amiiby.WebLAV.E2E`) を、版の4つ目だけ違う2つで作る (`target/msix-e2e/`。`scripts/msix.mjs --e2e`)。更新の確認は、古いほうを入れてから新しいほうで上から入れる。
- **常用の WebLAV とは別のパッケージで動かす**。確かめ用のパッケージは、名前と表示名だけが常用の版と違う。データ (パッケージごとの場所)・StartupTask・ファイアウォールの規則はパッケージごとに分かれるので、常用の版のものには触れない。テストのファイルごとに入れ直し、終わったら外す。ポートは 18370 にする。
- **回している間は常用の WebLAV を止める**。トレイのアイコンの名前が同じで見分けられないため。終わったら、動いていたなら起動し直す。止めるのは強制終了 (`Stop-Process -Force`)。
- **画面のロックを解いて流す**。回している間は画面を消さない。
- **マウスを使う**。トレイのメニューの項目は、位置を読んで本物のマウスで押す。回している間はキーボードとマウスに触らない。フォルダー選択の窓は窓のメッセージで操作し、キーは打たない。エクスプローラーの窓が開いて閉じる。
- **UAC の確かめの窓が出る** (`wack`・`nas`)。WACK は管理者の権限が要るので、管理者でなければ確かめの窓を出し、管理者の PowerShell で流す。数分かかる。報告は `target/e2e-msix/wack-report.xml`。
- **NAS の共有** (`nas`) は、環境変数 `WEBLAV_E2E_SMB` に手元の SMB の共有 (`\\<ホスト>\<共有>`) を書いたときだけ回す。共有の直下に確かめ用のフォルダー (`weblav-e2e-<ランダムな6文字>`) を作って消し、空いているドライブの文字に一時的に割り当てる。NAS が戻ったときの確認では、この PC から NAS への SMB をファイアウォールの規則で1分ほど止める (管理者の権限が要るので UAC の確かめの窓が出る。規則 `WebLAV-E2E-SMB-block-<回の印>` は、確かめが落ちても3分で外れる。PC を眠らせる・電源を切るなどで外れずに残ったら、管理者の PowerShell で `Remove-NetFirewallRule -Name 'WebLAV-E2E-SMB-block*'` で外す)。確かめ用のファイルは、一覧で中を読まれない拡張子にする (中を読んだ直後に止めると、NAS の側にファイルが開いたまま残り、確かめ用のフォルダーを消せなくなるため。Nano と Buffalo LinkStation の SMB 1 で、WebLAV を使わずに読んでも起きた)。途中で止まった回が残したフォルダーは、次の回の始めに消す。
- **電源の切れた NAS** (`offline`) は、返事をしない宛先 (`\\192.0.2.1\share`) を共有にして、手元のフォルダーが待たされないことを見る。登録の口は今つながるフォルダーしか受け付けないので、止めている間に DB で共有のパスを書き換える。NAS を待つので20秒ほどかかる。
- **残るもの**: ごみ箱に、消したものと上書きしたもの (`weblav-e2e-<時刻>`・`weblav-e2e-overwritten-<時刻>`) が入る。多重起動の確認では、2つ目の起動が既定のブラウザーで確かめ用の WebLAV の画面を開き、そのページが残る。共有にしたフォルダーは `target/e2e-msix/` に作る (AppData の下に作ると、パッケージの中から書いたものが外から見えないため)。

## Linux の確認

```
just linux-check-vm arch-hyprland          # VM の Hyprland で linux/*.test.mjs を全部
just linux-check-vm ubuntu-2604 tray       # VM の GNOME 50 で linux/tray.test.mjs だけ
just linux-check-vm ubuntu-2404 --deb <ファイル>   # 指した .deb で回す
```

見るもの: トレイのメニュー (`tray`)、多重起動 (`launch`)、ログイン時に起動と、デスクトップの設定で止めたとき (`login-item`)、フォルダー選択の窓とごみ箱 (消す・上書き) (`folders`)、NAS の共有と取り外した後のつなぎ直し (`nas`)。

- **VM の中で回す**。手元の画面・キーボード・フォーカスは使わない。VM は `e2e/vm/vm.sh` が Ubuntu・Arch の cloud image から作る (`e2e/vm/vm.sh <名前> create`。ディスクは `~/VMs/weblav/<名前>`、cloud-init で道具まで入れる。要るのは qemu・KVM と `~/.ssh/id_ed25519.pub`)。
  - `arch-hyprland`: Arch と Hyprland (トレイは waybar、フォルダー選択の窓は xdg-desktop-portal-gtk)。Hyprland は 3D 描画が要るので、virgl を `egl-headless` で使う。
  - `ubuntu-2404`・`ubuntu-2604`: GNOME 46・50 (トレイは Ubuntu の AppIndicator の拡張)。
  - VM が止まっていれば画面なしで起動し、回し終えたら止める (`--keep` で動かしたまま)。画面つきで見るときは `e2e/vm/vm.sh <名前> start`。
- **配る .deb を入れて回す**。今のコミットで `release-linux` の workflow が作った .deb を `gh` で落として (`target/linux-vm/<コミット>/`)、Ubuntu は apt で、Arch は AUR と同じ PKGBUILD (`packaging/aur/PKGBUILD.in`) で作ったパッケージを pacman で入れる。どちらも依存を引いて入れ、実行ファイルが読む共有ライブラリが揃っているかも見る。
  - .deb を作った回が無ければ、push してから `gh workflow run release-linux.yml --ref <ブランチ>` で作る (PR で workflow や `packaging/` を変えたときは、PR の回が作る)。
  - .deb に入る変更 (`e2e/` と文書のほかの変更) をコミットしていなければ、.deb と食い違うので止まる。
- **確かめ用の WebLAV はデータの置き場を分ける**。データの置き場 (`WEBLAV_HOME`) を `~/weblav-e2e` に、ポートを 18370 にして動かす。トレイの文言は、起動するときの `LANG` で日本語と英語を出し分けて見る。
- **トレイ**は D-Bus の StatusNotifierItem と dbusmenu で、メニューを読んで項目を押す。
- **フォルダー選択の窓**は portal が出すので、ほかのアプリの窓 (Hyprland は foot、GNOME はテキストエディター) を前面にしてから窓を出させ、前面が替わることと、ydotool で場所の欄 (Ctrl+L) に打ったパスが選ばれることで見る。前面の窓は、Hyprland は hyprctl、GNOME はアクセシビリティ (AT-SPI。`lib/atspi.py`) で読む。
- **多重起動**で2つ目が開くブラウザーは、開いた URL をファイルに書くだけの代わりにする (`x-scheme-handler/http` の既定を書き換える)。VM の中に窓が残り、続くテストの前面を乱さないため。
- **NAS の共有**は、VM の中の samba が guest で出す `smb://localhost/e2e` を、gio (gvfs) でつなぐ。保存したパスワードでつなぐ場合と同じく、パスワードを聞かれずにつなぎ直せる共有として使う。
- **残るもの**: VM のごみ箱に、消したものと上書きしたものが入る。ログイン時に起動の .desktop は、回す前の状態に戻す。
