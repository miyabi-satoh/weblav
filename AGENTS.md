# AGENTS.md

## プロジェクトルール

- shadcn component の改変はコードにコメントを残す。
- このリポジトリは公開する。コミットメッセージ・PR 本文・リポジトリの文書は `public-repo-writing` skill に従って書く。
  要件・画面ごとの仕様・運用の決まりは、このリポジトリに置いていない。
  技術の説明は `docs/<テーマ>.md` に、テーマごとに短く書く。
- フロントエンドの画面を追加・変更する作業は、着手前に docs/ui.md「見た目の決めごと」と、
  デザインキャンバス (非公開) の両方を見る。
  実装が食い違ったまま溜まるのを防ぐため、画面ができてからではなく着手前に見る。
  判断の根拠は docs/ui.md 側にあり、キャンバスは画面ごとの具体を確認するための補助。
  キャンバスを開けない場合は docs/ui.md「見た目の決めごと」だけを根拠に実装を進め、キャンバスを開けないことをユーザーに伝える。

## コードの規約

この節は「フォーマッタ・リンタが強制しないが、このコードベースが既に採用している書き方」をまとめたもの。
`cargo fmt` / `cargo clippy -D warnings` / `prettier` / `eslint` / `svelte-check` (まとめて `just ci`) が強制する項目はここに書かない。

新規実装は既存コードの中の似た処理を探し、その書き方に合わせること。

### Backend (Rust)

- **エラー型**: `AppError` (`src/error.rs`) の1箇所に集約する。
  バリアントは実際にそれを生成する箇所ができた時点で追加する (投機的に増やさない)。
  各バリアントは `status_and_code()` で `(StatusCode, 機械可読な code 文字列)` に対応させる。
  この `code` は frontend が表示文言を引くための契約なので、変更時は frontend 側の対応 (→ i18n の節) も確認する。
- **リクエスト抽出**: JSON ボディは `axum::extract::Json` ではなく `AppJson` (`src/error.rs`) を使う。
  抽出失敗時も共通の エラー envelope で返すため。
  `Path` / `Query` / `Multipart` は現状 axum のものをそのまま使っている (抽出失敗の頻度が低く、ラッパーを作る動機がまだ無い)。
  同種のラッパーが必要になったら `AppJson` に倣って `src/error.rs` に追加する。
- **API 定義**: 各ハンドラに `#[utoipa::path]` を付ける。
  ステータスコードごとの日本語説明は今後の新規実装で付ける方針とする (既存ハンドラには説明の無いものがあり、遡及適用はしない)。
  JSON フィールドは `#[serde(rename_all = "camelCase")]` で camelCase にする。
  リクエスト/レスポンスの型を変更したら `just api-types` を実行し、生成される `openapi.json`・`frontend/src/lib/api/schema.d.ts` の差分もコミットする (frontend の型が API の実装と静かに乖離しないようにするため)。
- **DB 操作**: `sqlx::query!` / `query_scalar!` 等のコンパイル時検査マクロを使う。
  SQL を変更したら `just sqlx-prepare` を実行し `.sqlx/` の差分もコミットする。
  **これを検知するのは lefthook のみ** (再生成に DB 接続が要るため `just ci` には含めていない)。
- **マイグレーション**: `migrations/` に1変更1ファイル。
  ファイル冒頭に、その変更をなぜ行うか・型ごとの必須カラムをどこで検証するか等をコメントで残す (既存ファイルがこの書き方をしている)。
  SQLite の制約でテーブル再作成が要る場合は、再作成するインデックスも同じファイルに書く。
- **テスト**: 単体テストは実装ファイル内に `#[cfg(test)] mod tests` で同居させ、HTTP 経由の統合テストは `tests/api/<機能>.rs` に置く (入口の `tests/api/main.rs` には、2つ以上のファイルで使う補助だけを置く)。
  - ログイン (401)・管理者の権限 (403)・この PC の中からだけの口 (LAN から 404) は、`tests/api/guards.rs` の API の一覧の表で確かめる。
    API を足したら、どれかに当たるものはその表に足す。口ごとの個別のテストは書かない。
  - 入力の検証・導出・並べ方のような決まりは、ハンドラの外にあるその関数の単体テストで場合ごとに確かめる (DB が要るものは `#[sqlx::test]`)。
    `tests/api/` には、その決まりが入口につながっていることを1件と、エラーの code (`assert_error`) を置く。
    決まりがハンドラの中に埋まっていて単体で呼べないものは、`tests/api/` で場合ごとに確かめる。
  DB が絡むテストは `#[sqlx::test]` を使う (`src/auth.rs` が例)。
  失敗時に原因が分かるよう、今後は `.unwrap()` ではなく `.expect("失敗理由")` を使う (既存テストには `.unwrap()` が残っている。
  遡及適用はしない)。
  テスト名は `authenticate_succeeds_with_correct_password` のように、何を検証しているかが読んで分かる文にする。
  テスト用の一時ディレクトリは `src/test_support.rs` のヘルパーを使う
  (lib・bin クレートと統合テスト (`tests/api/main.rs`) から `#[path]` で同じファイルを読み込んで共有している。
  `cfg(test)` はクレートごとに閉じるため `pub(crate)` では越えられない)。
- **ブロッキング I/O**: ファイルシステム操作 (`canonicalize`・`read_dir` 等) と Argon2 のハッシュ計算は `tokio::task::spawn_blocking` に載せる。
  件数に比例して重くなる CPU の計算 (アーカイブの全アイテムの導出など) も同じ。`crate::error::run_blocking` を通す。
  tokio のランタイムを止めないため。
- **バックエンドが出す文字列は英語で統一する**。`tracing` のログ・標準エラー出力・CLI の
  出力に加えて、**API のエラー応答の `message` も英語**にする。
  翻訳を持つのはフロントエンドだけで、表示言語を知っているのがブラウザー側だけのため
  (→ docs/architecture.md「エラーの契約と表示言語」)。利用者に見せる文言は、画面側が組み立てる。
  **例外はタスクトレイのメニュー・ツールチップ (`src/tray/`)**。最初に触るのがトレイであり
  (共有フォルダーの設定より前)、ブラウザーを開く前に表示言語を伝える手段がないため、
  OS の表示言語を直接判定して日本語/英語を出し分ける (→ docs/architecture.md「タスクトレイの表示言語」)。

### Frontend (Svelte)

- Svelte 5 runes のみを使う (`$props()` / `$state()` / `$derived()` 等)。
- **複数の画面で共有する Tailwind の class は `$lib/*.ts` に定数として置く**
  (`list-row.ts`・`breadcrumb.ts`・`header-action.ts`・`table-columns.ts`・`viewer-button.ts`)。
  同じ長い class 文字列を画面ごとに書かない。
- **Tailwind の任意値クラス (`text-[13px]`・`h-(--x)`・`[mask-type:luminance]` など) は使わない**。
  近い標準の段階に丸める (→ docs/ui.md「UI 全般」)。
  標準のユーティリティが無いプロパティは、`routes/layout.css` に `@utility` で名前を付けて使う (`touch-callout-none` が例)。
  `just lint` で `scripts/check-tailwind-arbitrary.mjs` が検出する。
  `data-[size=default]:` のような任意値の variant と、shadcn-svelte の部品 (`components/ui/`) は対象外。
- **クエリを持つ URL は `$lib/href.ts` の `withQuery()` で組み立てる**。
  エスケープの仕方と、空のときに `?` を付けるかを画面ごとにばらけさせない。
  SvelteKit のルートだけでなく、API への直リンク (`$lib/api/urls.ts`) やログインの戻り先も同じ。
  - 値が空の項目も**そのまま載る**。落としたい項目は呼び出し側で入れないこと
    (アーカイブの絞り込みのように、空の値に意味がある画面があるため)。
  - `resolve()` は `resolve('/x?a=1', params)` の形でクエリも受け取れるが、リテラルの `?` が要り、
    クエリの有無で呼び分けることになるので使っていない。
    クエリは base path の解決に関わらないので、解決済みのパスに後から足しても結果は変わらない。
  - href をヘルパー関数で組み立てると、`svelte/no-navigation-without-resolve` は
    `resolve()` まで追えないので使う側で抑制する。
    関数の中で `resolve()` をどう呼んでも、関数越しである限り同じように検出される。
    理由は毎回書かず、この規約を指すだけにする。
  - `<a>` の属性が1行に収まるときは `eslint-disable-next-line` を使う。
    複数行になると `href` の行を指せないので、その要素だけを `eslint-disable` / `eslint-enable` で囲む。
    囲む範囲を複数の要素にまたがらせない。
- **フォームの dirty 判定は `$lib/form-dirty.svelte.ts` の `FormDirtyState` を使う**。
  今の値を返す関数を渡し、開いた時点で `markPristine()` を呼ぶ。開いた時点のスナップショットと構造比較する形に揃える (値ごとの比較にしない)。
- 双方向バインディングは `bind:value` (または `bind:value={getter, setter}`) を使う。
  `value={x} onchange={...}` のような手動同期で代替しない。
- shadcn-svelte 由来のコンポーネントをラップする場合、`type Props = ButtonProps & {...}` のように既存 Props を拡張し、使わない props は `...restProps` でそのまま下層に渡す。
- shadcn component そのものを改変した場合は、改変理由をコード上にコメントで残す (→ プロジェクトルール)。
- **`placeholder` を使わない**。
  ラベル代わりにも入力例の提示にも使わない。入力を始めると消えるので、何を入れる欄か・どう書くかが見えなくなる。
  入力例は補助テキストとして要素の外に出す。
- ボタンラベルの表記規約 (体言止め/「...」/「〜する」) は `docs/ui.md`「UI 全般」を参照。

### i18n (Paraglide)

- キーは**フラットな snake_case** (Paraglide のメッセージキーがそのまま関数名になるため、ネストしたオブジェクトは使わない)。
- 命名パターンは `<画面 or コンポーネント名>_<要素>_<種別>`。
  - 画面名は route 名 (`login`・`admin` 等) かコンポーネント名を使う
  - 種別のサフィックス例: `_label`・`_button`・`_title`・`_description`・`_error_title`
  - 例: `login_username_label`・`admin_contents_nav_title`・`contents_form_confirm_button`
- 複数の画面・コンポーネントで同じ文言を使うものは、**横断プレフィックスにまとめて重複させない**。
  文言が画面ごとに変わるもの (「戻る」「このフォルダーにする」等) は、同じ役割のボタンでも画面側のプレフィックスに置く。
  新しい横断プレフィックスは、実際に重複が出た時点で足す。
  現状は次の7つ。
  - `error_` — API エラーの表示文言 (下記)
  - `contents_` — コンテンツ関連
  - `action_` — 画面によらず同じ汎用の操作ラベル (`action_cancel`・`action_close`)
  - `common_` — 画面によらず同じ汎用の文言のうち、ラベル以外 (`common_error_title`)
  - `site_settings_` — サイト設定関連 (ホームの見出しと管理画面のサイト設定の両方で使う)
  - `account_` — 自分のアカウントの操作 (ユーザーメニューのダイアログ) で共有する文言 (`account_current_password_label`)
  - `media_` — 音声のプレイヤーと動画のビューアーで共有する再生の操作 (`media_play_button`)
- **API エラーの表示文言は `error_<code>` に統一**し、`AppError::status_and_code()` が返す `code` 文字列とそのまま対応させる。
  例: `code: "not_found"` → キー `error_not_found`。
  個々の API 呼び出し側で文言を作り分けない。
  共通のエラーハンドラ (`$lib/api/errors.ts`) で `error_${code}` を引き、該当キーが無ければ `error_generic` にフォールバックする。
- **422 の内訳 (`error.detail`) の文言も、同じ共通のエラーハンドラで組み立てる** (→ docs/architecture.md「エラーの契約と表示言語」)。
  `kind` で場合分けし、サーバーが渡した値を差し込む。呼び出し側は今までどおり
  `errorMessage(error)` を呼ぶだけでよい。
  キーは `error_detail_<kind の snake_case>`。
  ただし**同じ文言がフォームの入力チェックにも要る場合は、そちらのキーを使い回す**
  (公開範囲の制約は `contents_form_visibility_*`)。同じ文言を2つ持つ方が害が大きいため。
- 日本語と英語の両方を同時に足す。
  片方だけ足してもう片方を後回しにしない。

### マニュアル (`docs/manual/`)

- 原典は日本語 (`ja/`)。訳を足すときは同じファイル名で `en/` に置く (→ `docs/help.md`)。
- **画面の表示言語を足すときは、マニュアルの訳も同時に足す**。
  マニュアルは画面の表示言語に従って出すので、訳の無い言語があると、その言語の利用者だけが
  日本語のマニュアルに落ちる。画面側だけ先に増やさない。
  (メッセージの ja / en を同時に足すのと同じ理由。)
- 他のページへのリンクは原典のファイル名で書く (`[設置する](02-setup.md)`。→ `docs/help.md`)。

## マージまでの流れ

共通の流れ (→ 共通の AGENTS.md「git の運用」) に、このリポジトリでは次が加わる。

- push 時、lefthook の pre-push フック (→ `just install`) が整形 (`just fmt-check`) を確かめる。
- PR を作ると、GitHub Actions の CI が `just ci` を流す。
