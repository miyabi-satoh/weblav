set windows-shell := ["cmd.exe", "/c"]

frontend_dir := "frontend"
# git 管理外の data/ (開発用の DB・設定・テスト用のコンテンツ)。git worktree の中からでも元の clone のものを使う。
# worktree ごとに写すと、DB と設定が分かれるため (frontend/scripts/data-dir.ts と同じ)。
data_dir := join(parent_directory(`git rev-parse --path-format=absolute --git-common-dir`), "data")
# sqlx のマクロと sqlx-cli が見る開発用の DB。.env の相対パスは worktree からは開けないので、絶対パスで上書きする。
# `sqlite://C:/...` だと URL の解析で `C:` がホストとポートに取られるため、スラッシュを付けない形にする
dev_db := data_dir / "dev-runtime" / "weblav.db"
export DATABASE_URL := "sqlite:" + dev_db
# DATABASE_URL があるとマクロは DB につなぎに行くので、DB をまだ作っていない clone では .sqlx/ のキャッシュを読ませる。
# 呼び出し側 (CI など) が渡した値はそのまま使う
export SQLX_OFFLINE := env("SQLX_OFFLINE", if path_exists(dev_db) == "true" { "false" } else { "true" })
bin_name := "weblav"

# 使えるコマンド一覧を表示
default:
    @just --list

# frontend の依存関係と lefthook の pre-commit/pre-push hook をインストールする
install:
    pnpm install
    cd {{ frontend_dir }} && pnpm install
    cd account-server && pnpm install
    cd site && pnpm install
    cd {{ frontend_dir }} && pnpm exec playwright install chromium

# SvelteKit(SPA) をビルドして frontend/build/ に出力
frontend-build:
    cd {{ frontend_dir }} && pnpm run build

# frontend をビルドしてから release バイナリをビルド
build: frontend-build
    cargo build --release

# release ビルドしてバイナリを起動
[unix]
run: build
    ./target/release/{{ bin_name }}

[windows]
run: build
    .\target\release\{{ bin_name }}.exe

# assets/icon.svg・tray-icon-mac.svg から exe・タスクトレイ・メニューバー・MSIX・ブラウザのタブ用の画像 (assets/icon.ico・tray-icon-64.rgba・tray-icon-mac-64.rgba・msix/*.png・frontend の favicon.svg) を再生成する
generate-icon:
    pnpm run generate:icon

# Windows版の MSIX を作る (dist/weblav-v<version>.msix、要: Windows SDK)。試しに入れるための自己署名の証明書で署名する。
# ビルドはWindows 実機で手動で行う想定 (→ docs/distribution.md)。`ci`に依存させているのは、
# 配布物だけ`test`を素通りして壊れたまま渡してしまわないようにするため (タスクトレイの絵の
# サイズ不一致等はテストでしか検知しない、→ src/tray/mod.rsのtray_icon_asset_is_a_valid_square_rgba_buffer)。
# 証明書は先に installer/msix/new-test-cert.ps1 で作っておく (→ docs/distribution.md「MSIX (Windows)」)。
# Store に上げる版は `just msix --store` で、Store の発行元のまま署名せずに作る (dist/weblav-v<version>-store.msix)。
[windows]
msix *args: ci
    node scripts/msix.mjs {{ args }}

# macOS 版の .app を作る (target/release/bundle/WebLAV.app)。公証していないため、まだ配らない (→ docs/distribution.md「ビルド・配布の方法」)
[macos]
bundle-mac: build
    node scripts/bundle-mac.mjs

# 動いている WebLAV は SIGTERM で止める (トレイは受けて片付けないので、処理中のリクエストは切れる)。
# macOS 版の .app を作って /Applications に入れ直し、起動し直す
[macos]
install-mac: bundle-mac
    -pkill -U "$(id -u)" -x weblav
    for _ in $(seq 50); do pgrep -U "$(id -u)" -x weblav >/dev/null || exit 0; sleep 0.2; done; echo "WebLAV が10秒たっても終わりません" >&2; exit 1
    rm -rf /Applications/WebLAV.app
    cp -R target/release/bundle/WebLAV.app /Applications/
    open /Applications/WebLAV.app

# frontend/build が無ければビルドする (内部用)
[private]
ensure-frontend-build:
    @{{ if path_exists(justfile_directory() / frontend_dir / "build") == "true" { "echo frontend/build exists" } else { "just frontend-build" } }}

# 開発版のサーバーを起動する (DBはdata/dev-runtime/配下、Ctrl+Cで止める)。
# トレイは出さずに済むよう、サーバーだけの `weblav-service` を動かす
[unix]
dev-backend: ensure-frontend-build
    WEBLAV_HOME="{{ data_dir }}/dev-runtime" cargo run --bin weblav-service

[windows]
dev-backend: ensure-frontend-build
    set "WEBLAV_HOME={{ data_dir }}\dev-runtime" && cargo run --bin weblav-service

# 開発版のCLIを引数付きで起動する。--create-user/--openapi/-v等を渡す (DBはdev-backendと同じ)
[unix]
dev-cli *args: ensure-frontend-build
    WEBLAV_HOME="{{ data_dir }}/dev-runtime" cargo run --quiet --bin weblav-cli -- {{ args }}

[windows]
dev-cli *args: ensure-frontend-build
    set "WEBLAV_HOME={{ data_dir }}\dev-runtime" && cargo run --quiet --bin weblav-cli -- {{ args }}

# 窓口 (account-server/) と紹介のサイト (site/) を手元で起動する。D1 は手元のもの (account-server/.wrangler) を使い、メールは送らずリンクをログに出す。
# 開発版の WebLAV は、ここ (127.0.0.1:8787) に申し込む
dev-account-server:
    cd account-server && node -e "require('fs').existsSync('.dev.vars') || require('fs').copyFileSync('dev.vars.example', '.dev.vars')"
    cd site && pnpm run build
    cd account-server && pnpm exec wrangler d1 migrations apply DB --local
    cd account-server && pnpm exec wrangler dev --ip 127.0.0.1 --port 8787 --local-upstream 127.0.0.1:8787 --var MAIL_LOG_ONLY:1

# 手元の窓口で、アカウントに Pro を付ける (買う流れの代わり)。先にそのアドレスでサインインしておく
dev-account-grant email plan="personal":
    cd account-server && pnpm exec wrangler d1 execute DB --local --command "INSERT INTO subscriptions (id, account_id, plan, paid_through, status, created_at) SELECT 'dev-' || lower(hex(randomblob(8))), id, '{{ plan }}', unixepoch('now', '+1 year'), 'active', unixepoch() FROM accounts WHERE email = lower('{{ email }}')"

# frontend を開発モードで起動 (HMR, /api は backend にプロキシ)
dev-frontend:
    cd {{ frontend_dir }} && pnpm run dev

# backend/frontend をまとめて起動する (concurrently でラベル付き・1ターミナルに集約)
dev:
    cd {{ frontend_dir }} && pnpm exec concurrently -n backend,frontend -c blue,green "just dev-backend" "just dev-frontend"

# E2Eブラウザテストを実行する。backendの起動方法は複数あり一つに決め打てないため、
# 事前に起動しておくこと (`just dev-backend`等)。e2e はテストごとにコンテンツを作り Free の上限に当たるので、
# debug ビルドの backend の WEBLAV_HOME に tests/fixtures/dev-pro.json を pro.json として置いておく
# (release ビルドはこの証明を信じない。→ docs/pro.md「結び付きと許可」)。管理者アカウントでの
# ログインを検証するため、そのユーザー名・パスワードを渡す (ユーザー名省略時は"admin")。
# 例: just e2e http://127.0.0.1:3000 P@ssw0rd
# 引数は `$` 付きで環境変数として渡す。コマンド文字列に埋め込むと、Windows の cmd.exe が
# パスワード中の `%` や `"` を解釈して値や構文が壊れるため。
e2e $E2E_BASE_URL $E2E_ADMIN_PASSWORD $E2E_ADMIN_USER="admin":
    cd {{ frontend_dir }} && pnpm run test:e2e

# E2Eブラウザテストを、使い捨てのbackend (一時WEBLAV_HOME・空きポート・管理者1人) に対して実行する。
# 開発用DBを使わないので、途中で落ちても汚さない。`just ci` (CI) から呼ばれる
e2e-local: ensure-frontend-build
    cargo build --bin weblav-service
    cd {{ frontend_dir }} && node scripts/run-e2e.ts

# sqlx::query! 系マクロのオフラインキャッシュ(.sqlx/)を再生成する (クエリ変更時に実行してコミットする)。
# DB は just dev-backend と同じもの (DATABASE_URL)
sqlx-prepare: ensure-frontend-build
    cargo sqlx prepare -- --all-targets

# .sqlx/ が最新か確認する (再生成して差分・未追跡ファイルが無いことを見る)。pre-commit hook から呼ばれる
sqlx-prepare-check: sqlx-prepare
    node scripts/check-generated.mjs sqlx-prepare .sqlx/

# OpenAPI仕様(openapi.json)をサーバー起動なしで生成する
openapi: ensure-frontend-build
    cargo run --quiet --bin weblav-cli -- --openapi > openapi.json

# openapi.json から frontend 用の TypeScript 型を生成する
api-types: openapi
    cd {{ frontend_dir }} && pnpm run generate:api-types

# openapi.json / schema.d.ts が最新か確認する (再生成して差分が無いことを見る)
# API変更時に再生成を忘れるとレビュー・型検査どちらでも気づけない
# (既存フィールドの変更・削除でも型エラーにならない)ため、専用のチェックが要る。
# pre-commit hook(lefthook)と `just ci` (CI が流す) の両方がこのレシピを呼ぶ。
api-types-check: api-types
    node scripts/check-generated.mjs api-types openapi.json frontend/src/lib/api/schema.d.ts

# 配布物に入る Rust の依存のライセンス表示を作る (要: cargo-about。→ docs/third-party-licenses.md)
licenses:
    node scripts/generate-rust-licenses.mjs

# ライセンス表示が最新か確認する。依存を変えたら `just licenses` の結果をコミットする
licenses-check: licenses
    node scripts/check-generated.mjs licenses frontend/static/third-party-licenses/rust.json

# ビルドより前に、原稿のフォルダの指定を確かめる (相対パスはリポジトリの直下から)
_spec-dir:
    @node -e "const d = process.env.WEBLAV_SPEC_DIR; if (!d || !require('fs').statSync(d, { throwIfNoEntry: false })?.isDirectory()) { console.error('WEBLAV_SPEC_DIR に仕様書の原稿のフォルダを指定してください'); process.exit(1); }"

# アプリ仕様書を生成してmoに追加する (ブラウザは開かない。要: mo。原稿のフォルダを WEBLAV_SPEC_DIR で渡す)
spec: _spec-dir frontend-build
    cargo build
    cd {{ frontend_dir }} && node scripts/generate-spec.ts
    mo -R docs/generated/spec/ --target weblav-spec --no-open

# コードを整形する (cargo fmt + prettier)
fmt:
    cargo fmt
    cd {{ frontend_dir }} && pnpm run format
    cd account-server && pnpm run format

# フォーマット崩れがないか確認する (pre-push hook から呼ばれる。書き換えない)
fmt-check:
    cargo fmt --check

# Lint を実行する (clippy + eslint/prettier check)
lint: ensure-frontend-build
    cargo clippy --all-targets -- -D warnings
    cd {{ frontend_dir }} && pnpm run lint
    node scripts/check-tailwind-arbitrary.mjs

# 窓口 (account-server/) を検査する (整形・生成した型・型検査・テスト)
account-server-check:
    cd account-server && pnpm run lint
    cd account-server && pnpm exec wrangler types --check
    cd account-server && pnpm run check
    cd account-server && pnpm run test

# 紹介のサイト (site/) を検査する (型検査・ビルド)。ビルドしたものは窓口の Worker が出す (→ account-server/wrangler.jsonc)
site-check:
    cd site && pnpm run check
    cd site && pnpm run build

# 型検査を実行する (cargo check + svelte-check)
check: ensure-frontend-build check-frontend
    cargo check

# フロントエンドだけ型検査する。`ci` はこちらを使う (Rust の型検査は `lint` の clippy が兼ねるので、二重に走らせない)
check-frontend: ensure-frontend-build
    cd {{ frontend_dir }} && pnpm run check

# テストを実行する (cargo test + vitest)
test: ensure-frontend-build
    cargo test
    cd {{ frontend_dir }} && pnpm run test:unit -- --run

# 一連の品質チェック (フォーマット→lint→型検査→生成物→test→build→e2e)。
# CI (GitHub Actions) が PR ごとに流す。e2e は build の後に置き、その時点のフロントで流す
ci: fmt-check lint check-frontend api-types-check licenses-check test account-server-check site-check build e2e-local

# ビルド成果物を削除する
[unix]
clean:
    cargo clean
    rm -rf {{ frontend_dir }}/build {{ frontend_dir }}/.svelte-kit

[windows]
clean:
    cargo clean
    if exist {{ frontend_dir }}\build rmdir /s /q {{ frontend_dir }}\build
    if exist {{ frontend_dir }}\.svelte-kit rmdir /s /q {{ frontend_dir }}\.svelte-kit
