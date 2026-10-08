# 開発

ビルド・起動・テスト・配布物の作り方。

## 技術スタック

- backend: Rust + axum
  - DB: SQLite (sqlx)
  - 設定: `config.toml` + `directories`
  - ロギング: tracing
  - 認証: tower-sessions + argon2
  - OpenAPI 仕様生成: utoipa
  - タスクトレイ常駐: tao + tray-icon
  - frontend の埋め込み配信: rust-embed
- frontend: SvelteKit (SPA モード, `adapter-static`) + TypeScript
  - UI: shadcn-svelte (bits-ui) + Tailwind CSS v4
  - API クライアント: openapi-fetch + openapi-typescript
  - i18n: Paraglide (`en` / `ja`)
  - テスト: vitest (unit) + Playwright (e2e)
- パッケージ管理: pnpm (frontend)

## セットアップ

```sh
just install   # frontend の依存関係と lefthook の pre-commit/pre-push hook をインストール (pnpm)
```

第三者のライセンス表示を作るのに cargo-about が要る (`just licenses` / `just licenses-check`。
後者は `just ci` から呼ばれる)。

```sh
cargo install --locked cargo-about@0.9.2 --features cli   # `--features cli` が無いとコマンドが入らない
```

`just install` で lefthook の hook が有効になる。
`git commit` 時に `src/**/*.rs`(・`migrations/**/*.sql`)の変更があれば
`openapi.json` / `schema.d.ts` / `.sqlx/` の再生成漏れを、`Cargo.lock`・`about.toml`・`license-texts/`・生成と整形のスクリプトの変更があればライセンス表示の生成漏れを自動検知する。
`git push` 時には整形 (`just fmt-check`) だけを確かめる。
`just ci` (フォーマット→lint→型検査→生成物→test→build→e2e) は、GitHub Actions の CI が PR ごとに流す (`.github/workflows/ci.yml`)。

## 開発

```sh
just dev           # backend + frontend をまとめて起動 (ラベル付きで1ターミナルに出力を集約)
just dev-backend   # backend だけ起動 (サーバーだけの exe `weblav-service` で動かし、トレイは出さない。初回のみ frontend をビルド)
just dev-frontend  # frontend だけ起動 (HMR 付き、/api は backend にプロキシ)
```

- `WEBLAV_HOME` に元の clone の `data/dev-runtime/` がセットされる。
  git worktree の中でも、`just` のコマンドは元の clone の `data/` を使う (`justfile` の `data_dir`)。
- CLI は `just dev-cli <引数>` で開発版の `weblav-cli` に渡す (例: `just dev-cli --create-user admin --admin`)。
- clone・pull したら、`just check`・`just ci` の前に一度 `just dev` で起動できることを確かめる。
  依存の入れ忘れにも気づけるうえ、paraglide の生成物 (`frontend/src/lib/paraglide`、Git 管理外) が作り直される。
  この生成物は vite が起動したときにしか作られず、古いままだと、足したメッセージのキーが見つからず `check` が止まる。
- 開発用 DB を作ってある環境で、pull して `migrations/` が増えていたら、`just dev` の前に元の clone で `cargo sqlx migrate run` を流す (sqlx-cli と `.env` は下の「sqlx (コンパイル時クエリチェック)」)。
  `sqlx::query!` はビルドの時点で開発用 DB を見るので、未適用のマイグレーションがあると `no such table` でビルドが止まる。
  起動時のマイグレーションはビルドの後なので、`just dev` では当たらない。

### sqlx (コンパイル時クエリチェック)

`sqlx::query!`/`query_as!` は SQL をコンパイル時に検証する。
検証結果は `.sqlx/` にキャッシュされる。

クエリを追加・変更したときは開発用 DB が要る。
pre-commit hook(lefthook)が `src/**`(・`migrations/**`)の変更のたびに
`just sqlx-prepare` を実行して差分を確認するため、早めに作っておくとよい。
`.env` は `cargo sqlx` を直に流すときに使う (`just` のコマンドは `justfile` の `DATABASE_URL` を使う):

```sh
# 次の2行は元の clone (git worktree でない方) で流す
cp .env.example .env
mkdir -p data/dev-runtime && cargo sqlx database setup  # migrations/ を適用した開発用 DB を作成
just sqlx-prepare                                       # .sqlx/ を再生成する (クエリ変更のたびに実行してコミット)
```

再生成漏れは pre-commit hook(lefthook)が検知する
(DB 接続が要るため `just ci` には含めていない)。
CI の `just ci` は `SQLX_OFFLINE=true` で流し (`.github/workflows/ci.yml`)、キャッシュでも検査する。

この DB (`data/dev-runtime/weblav.db`) は `just dev-backend` が使う DB と同じもの。
`.env` の `DATABASE_URL` は相対パスなので、git worktree の中では開けない。
`just` のコマンドは絶対パスで渡し直すので (`justfile` の `DATABASE_URL`)、worktree でもそのまま使える。
`cargo sqlx database setup`・`cargo sqlx migrate run` は元の clone で流す。
worktree で `cargo check` などを直に流すときは `SQLX_OFFLINE=true` を付ける。
マイグレーション追加は `cargo sqlx migrate add <name>`。sqlx-cli のインストールは
`cargo install sqlx-cli --no-default-features --features sqlite`。

### 注意すること

- **`.sqlx` の再生成漏れ**。
  スキーマを変えると、既存の `sqlx::query!` が全て再チェックの対象になる。
  `just sqlx-prepare` をコミットのたびに実行する。
  検知するのは lefthook だけ (→ docs/architecture.md「アーキテクチャ」)。
  マイグレーションを書いたら、先に `sqlx migrate run` で開発用 DB に当ててから `cargo check` する。
- **開発用の DB は `data/dev-runtime/weblav.db` の1つ**。
  sqlx のコンパイル時検査 (`DATABASE_URL`) と `just dev-backend` (`WEBLAV_HOME` 配下) が共用する。
- **適用済みのマイグレーションを書き換えたら、DB を消さないと起動しない**。
  sqlx は `_sqlx_migrations` にチェックサムを記録しており、内容が変わると
  `migration <version> was previously applied but has been modified` で止まる。
  手順は「起動中の WebLAV を止める → DB とその `-wal` / `-shm` を消す → 当て直す」。
  `data/dev-runtime/session.key` は消さなくてよい。
  ほかの環境に渡って適用されたマイグレーションは、編集も改名もせず、変更は新しいファイルとして足す (→ docs/architecture.md「既存テーブルの変更」)。
- **公開範囲の継承を取りこぼさない**。
  祖先の判定は、単体で開く全経路 (グループ・フォルダ・ファイル配信・アーカイブ) が `ensure_viewable` を呼ぶ (→ docs/access.md「祖先のグループを辿る」)。
- **公開範囲の比較を SQL に持ち込まない**。
  `visibility` は TEXT で保存しており、SQLite の文字列比較は `'authenticated' < 'public'` となる。
  レベルの順序 (`public` < `authenticated`) と逆になるため、`WHERE visibility < ?` は黙って期待と反対の結果を返す。
  順序の定義は `Visibility` の `Ord` ただ1つに保ち、比較は Rust 側で行う。
- **テストやショットの一時ディレクトリを `std::env::temp_dir()` / `tmpdir()` に置かない**。
  片付け漏れが目に付かない場所に溜まるため。
  Rust のテストは `test_support::project_temp_dir` か `CARGO_TARGET_TMPDIR`、`just spec` のフィクスチャは `target/` 配下を使う。
- **アーカイブの走査と更新の競合**。
  再スキャンの per-archive 排他を `path` の更新・削除にも掛ける。
  `path` を変更したら `archive_items` を全削除する。
  忘れると、変更前の公開フラグが変更後の無関係なファイルへ引き継がれる (→ docs/archive.md「スキャン」)。

## テスト

```sh
just test        # テスト実行 (cargo test + vitest)
just e2e-local   # E2E を使い捨ての backend (一時 WEBLAV_HOME・管理者1人) に対して実行する。開発用 DB は使わない
just ci          # fmt-check → lint → check-frontend → api-types-check → licenses-check → test → build → e2e-local を一括実行 (CI が流す)
```

## ビルド

```sh
just build  # frontend をビルドしてから release バイナリをビルド
just run    # build してバイナリを起動
```

サインアップ UI は無い。最初の管理者は、トレイの「セットアップ」で開く画面 (`/setup`) で作る (開発中は `just dev-cli --create-user <name> --admin` でもよい)。以降のユーザーは管理画面 (`/admin/users`) から足す。
管理者がパスワードを忘れたときは、リカバリコードでログイン画面から再設定する (→ docs/access.md「リカバリコード」)。

## 配布

(→ `docs/distribution.md`)

```sh
just msix         # Windows: dist/weblav-v<version>.msix を作る (build → makeappx、試しに入れるための自己署名つき)
just install-windows  # Windows: 試しの MSIX を作って上から入れ直し、起動し直す
just bundle-mac   # macOS: target/release/bundle/WebLAV.app を作る。公証していないので、まだ配らない
just install-mac  # macOS: .app を作って /Applications に入れ直し、起動し直す
```

- Windows は Microsoft Store の MSIX だけで配る。
- ビルドは Windows 実機で手動で行う (CI は配布物を作らない)。Windows SDK が要る。
- WebLAV はログインした人のタスクトレイに常駐し、サーバーを同じプロセスで動かす (→ `docs/distribution.md`「常駐 (Windows)」)。
- exe のバージョン・アイコンは `build.rs` (`winresource`) が毎回埋め込む
  (`[package.metadata.winresource]`、`Cargo.toml`)。
- アイコンの絵 (`assets/icon.svg`、macOS のメニューバー用は `assets/tray-icon-mac.svg`) を直したら `just generate-icon` で
  `assets/icon.ico`・`assets/tray-icon-64.rgba`・`assets/tray-icon-mac-64.rgba`・`assets/msix/*.png`・
  `frontend/src/lib/assets/favicon.svg` を作り直してコミットする
  (Node製のスクリプトなので Mac でも実行できる)。
- ダブルクリック起動 (タスクトレイ常駐) はコンソールを出さない (`windows_subsystem = "windows"`、リリースビルドのみ)。
  そのためコマンドプロンプトからは CLI として使えず、CLI はコンソール用の別 exe `weblav-cli` に分けてある (全 OS 共通)。
  `weblav-cli` は開発・テスト用で、リリース版 (MSIX・.app) には入れない。

## 設定とデータの置き場所

- `WEBLAV_HOME` が設定されていればそのディレクトリ直下
- 無ければ `directories` crate が解決する OS 標準のアプリデータディレクトリ配下
  - macOS: `~/Library/Containers/com.amiiby.weblav/Data/Library/Application Support/com.amiiby.weblav/` (.app はサンドボックスで動くため。`weblav-service` は `~/Library/Application Support/com.amiiby.weblav/`)
  - Linux: `~/.config/weblav/` (設定) / `~/.local/share/weblav/` (データ)
  - Windows: `%APPDATA%\amiiby\weblav\config\` (設定) / `%LOCALAPPDATA%\amiiby\weblav\data\` (データ)。
    MSIX で入れたときは、どちらも `%LOCALAPPDATA%\Packages\<パッケージ名>\LocalCache\{Roaming,Local}\` の下に回される (→ `docs/distribution.md`「MSIX (Windows)」)

`config.toml` が無ければ既定値で起動する。パス・項目・既定値は `config.example.toml` を参照。

## CLI

`weblav-cli` は開発・テスト用 (リリース版には入らない)。

```
weblav                                     タスクトレイに常駐し、サーバーを同じプロセスで動かす (引数は取らない)
weblav-service                             サーバーだけを動かす (Ctrl+C / SIGTERM まで動く。開発と e2e 用)
weblav-cli --create-user [<name>] [--admin]  ユーザーを作成する (名前を省くと対話入力、パスワードは常に対話入力)
weblav-cli --openapi                       OpenAPI 仕様 (JSON) を標準出力に書き出す
weblav-cli -v | --version                  バージョンを表示する
```

## API

`/api/v1` 配下。仕様は `openapi.json` (コミット対象)。
API を変更したら `just api-types` で型を再生成してコミットする。
再生成漏れは pre-commit hook(lefthook)と `just ci` (CI が流す) の両方で検知する。

エラーは常に `{"error":{"code":"...","message":"..."}}` の形で返る。
ただしこれは `fetch` から呼ぶクライアント向けの契約で、**ブラウザが URL を直接開く経路 (ファイルのダウンロード) だけは画面へリダイレクトする** (→ `docs/architecture.md`「エラーの契約と表示言語」)。

## その他のコマンド

```sh
just openapi     # openapi.json を生成
just api-types   # openapi.json から frontend 用の TypeScript 型を生成
just spec        # アプリ仕様書を生成して mo に追加する。ブラウザは開かない (要 mo)
just sqlx-prepare # sqlx::query! 系マクロのオフラインキャッシュ (.sqlx/) を再生成 (要 開発用 DB)
just licenses    # Rust の依存のライセンス表示 (frontend/static/third-party-licenses/rust.json) を再生成 (要 cargo-about)
just fmt         # コード整形 (cargo fmt + prettier)
just lint        # Lint (clippy + eslint/prettier check)
just check       # 型検査 (cargo check + svelte-check)
just check-frontend  # svelte-check だけ (just ci が使う。Rust の型検査は lint の clippy が兼ねる)
just clean       # ビルド成果物を削除
```

コマンド一覧は `just --list` でも確認できる。

### アプリ仕様書

原稿は公開していない (作者の手元にだけある)。そのフォルダを環境変数 `WEBLAV_SPEC_DIR` で渡す (相対パスはリポジトリの直下から)。
`just spec` でシード投入 → スクリーンショット撮影 → プレースホルダー置換のうえ `docs/generated/spec/` に生成する。
要: `mo`。
