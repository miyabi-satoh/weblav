# アーキテクチャとデータモデル

## アーキテクチャ

- Backend: Rust + Axum 0.8 + SQLite (sqlx) + tower-sessions
- Frontend: SvelteKit (Svelte 5 runes, SPA / `adapter-static`) + Tailwind CSS 4 +
  shadcn-svelte (`bits-ui`) + Paraglide JS (日本語/英語)
- API: REST、`/api/v1` プレフィックス。`utoipa` で OpenAPI を自動生成し、
  フロントは `openapi-typescript` + `openapi-fetch` で型付きクライアントを
  生成する。バックエンド/フロントの型が自動的にずれない構成。
- 配布: `rust-embed` が `frontend/build/` をバイナリへ埋め込み、単一バイナリで
  配布する。設定は `config.toml`。Windows では Microsoft Store から MSIX で入れ、タスクトレイに常駐する
  アプリがサーバーを同じプロセスで動かす (→ distribution.md「常駐 (Windows)」)。
- 認証は `tower-sessions` + SQLite store。パスワードは Argon2id
  (実装済み: `src/auth.rs`)。
- **スキーマ・ルートを変更したら `just` で OpenAPI と `.sqlx` を再生成する**。
  OpenAPI (`openapi.json` / `schema.d.ts`) の生成漏れは lefthook と
  `just ci` (CI が流す) の両方が、`.sqlx` の生成漏れは
  lefthook のみが検知する (`.sqlx` の再生成には DB 接続が要るため
  `just ci` には含めていない)。

### 土台にしている仕組み

- パストラバーサル防御: 保存時に `std::fs::canonicalize`、閲覧時に再度
  canonicalize して `starts_with` 比較 (実装済み: `validate_folder_path` /
  `resolve_path`)。TOCTOU の制約 (登録先に信頼できない書き込み主体を置かない)
  は、そのドキュメントコメントに書いてある。
- 循環参照の検出 (`WITH RECURSIVE ancestors`)、グループ削除時の
  「直下の子をルート直下へ昇格」 (実装済み: `validate_parent` /
  `delete_content`)。
- `file` コンテンツの content-addressed blob ストレージと参照0件時の GC
  (実装済み: `gc_blob_if_unreferenced`)。
- Range リクエスト対応 (`ServeFile` 経由。実装済み: `serve_file_response`)。

### エラーの契約と表示言語

API のエラーは共通の envelope で返す。
**この契約は `fetch` から呼ぶ API クライアント向けのもの**で、ブラウザーが URL を直接開く経路は別に扱う (下記)。

```jsonc
{
  "error": {
    "code": "invalid_request_body",   // 機械可読。画面はこれで文言を引く
    "message": "an axis named ... already exists",  // 英語。デバッグ用で画面には出さない
    "detail": {                        // 一部の 422 だけに付く
      "kind": "axisNameTaken",
      "name": "級"
    }
  }
}
```

- **バックエンドが出す文字列は英語で統一する**。CLI の出力・ログ・`message` の全て。
  翻訳はブラウザー側 (Paraglide) だけが持つ。表示言語を知っているのはブラウザーであり、
  サーバーに翻訳を持たせると同じ語を2箇所で管理することになるため。
- `code` だけでは「入力内容を確認してください」までしか言えない。
  **値を見せないと直しようがない 422 には `detail` を添える**
  (`src/api/error_detail.rs` の `ValidationDetail`)。`kind` で場合分けし、
  文言に差し込む値 (タイトル・軸名・公開範囲) を併せて渡す。画面は
  `frontend/src/lib/api/errors.ts` でこれを文言に組み立てる。
- **全ての 422 に付けるわけではない**。汎用の文言で用が足りるものはそのままにする。
  増やす基準は「利用者がその値を知らないと直せないか」。
- 未知の `kind` (サーバーだけ先に更新された場合) では、画面は `code` の汎用文言に落ちる。

#### ブラウザーが直接開く経路

ファイルは `<a href target="_blank">` で `GET /contents/{id}/download` をそのまま開く。
ここで envelope をそのまま返すと、タブに `{"error":{"code":"unauthorized",...}}` が出るだけで、そこから戻る手がかりが無い。
閲覧者にはタブレットの生徒が含まれるので、これは行き止まりになる。

**`Accept` が HTML を求めている要求にだけ、画面へのリダイレクト (303) を返す**。

| 状況 | 行き先 |
|---|---|
| 401 | `/login?redirect=<元のパス>`。ログインするとそのファイルが開く |
| 404 | `/?error=not_found` |
| その他のエラー | `/?error=generic` |

403 の分岐 (行き先は `/?error=forbidden`) も置いてあるが、**ダウンロードの経路では現状発生しない** (403 を返すのは書き込み系だけで、閲覧は 401 か 404 に寄せてある → access.md「公開範囲」)。
将来返すようになったときに JSON が漏れないよう、分岐だけ先に用意している。

- 判定は `Accept` に `text/html` が名指しされているかで行う。
  `fetch` の既定は `*/*`、`<audio>` や `<img>` は自分の型を並べるので、いずれもリダイレクトしない。
  画面の中で鳴らす・表示するものをページへ飛ばしても意味がないため。
- **行き先はステータスだけで決め、応答のボディは読まない**。
  `code` を取るには本文を読み切る必要があるうえ、ここで分けたい粒度は「ログインへ送るか、伝えて終わるか」しかない。
- 文言は画面側が `?error=` の値から組み立てる。翻訳を持つのはフロントエンドだけという方針 (上記) を崩さない。
- 掛けるのは**ダウンロード系のルートだけ**。他の API は今までどおり JSON を返す。
- 401 の戻り先は API のパスになる。**SvelteKit のルートではないので `goto` では解決できず**、ログインの成功後は `window.location` でブラウザーに開かせる。
  インライン表示できない型 (docx・zip 等) はダウンロードが始まるだけで文書は遷移しないので、先に `invalidateAll()` で画面の状態を更新しておく。
  ログイン済みで `/login?redirect=/api/...` を開いた場合は戻り先を捨ててトップへ送る (ファイルは開き直せるため、ここで凝らない)。
- **管理画面のアーカイブのアイテム一覧では、未公開のアイテムもタイトルをリンクにし、管理用の配信 (→ archive.md「アイテムの配信」) で開く**。
  辞書を決めるには、ダウンロードしてきたファイルのように名前だけでは分からないものの中身を確かめる必要がある。
  確かめるために公開すると、そのあいだ閲覧側にも出てしまう。

### LAN からの到達性

- **既定の `bind` は `0.0.0.0`**。
  設置してすぐタブレット・スマートフォンから開けるようにするため。
  想定する管理者は、手順書には従えるが設定ファイルの編集には習熟していないので、そこに `config.toml` の編集を挟めない。
- **既定は HTTP。HTTPS にするかは設置者が決める**。
  WebLAV 本体に HTTPS や証明書の仕組みは持たせず、リバースプロキシ (Caddy など) を前に置く構成でも動くようにしておく。
  - LAN の `.local` や IP には公開の証明書が付かない。自己署名 (全端末で警告)・独自 CA (端末ごとに信頼させる)・
    ベンダーのドメイン (発行基盤の運用とインターネットへの依存) のどれも、非技術者向けの既定には重い。
    非技術者向けの製品 (Plex・Unraid) は公開 CA の証明書を自動で配る方を採っており、そこまでは負えない。
  - 想定する家庭・小さな事務所の閉じたネットワークでは、HTTP の危険は受け入れる範囲。
    公共の Wi-Fi で動かさないことはマニュアルで案内する。今の機能 (閲覧・ダウンロード・音声再生・ログイン) は HTTP で足りる。
  - **考え直すのは**、Secure Context が要る機能 (PWA・Service Worker・クリップボード API) を入れたいときと、
    LAN の外から使う要望が出たとき。
- インターネットには公開しない。閉じたネットワーク (LAN) での運用を前提にする。
  HTTPS 終端を前段に置く構成 (上記) を採った運用者は、`session.secure_cookie` を自分で有効にする。
  **トレイがブラウザーで開くのは `http://localhost:<port>`** (`127.0.0.1` ではなく)。
  `secure_cookie` を有効にすると、Edge・Chrome は HTTP の `127.0.0.1` ではログインの Cookie を受け取らず、`localhost` だけを例外にするため (実測)。PC の前での管理 (→ access.md「初回セットアップ」・folders.md「公開できるフォルダー」) が HTTPS 化の後も続けられる。
  非ループバックで待ち受けながら無効のままであることは起動時に記録するが、
  **既定の構成で必ず通るので `warn` にはしない** (推奨どおり設置した管理者が毎回警告を見ることになり、ログ全体を無視させるため)。
- **サーバーのアドレスはタスクトレイのメニューにもツールチップにも出さない**。
  代わりに、共通ヘッダーの「接続」アイコン (→ ui.md「UI 全般」) から、mDNS のホスト名を案内する。
  - **DHCP で変わる IP アドレスではなく、変わらない名前を案内する**。
    ブックマークした URL が DHCP の再割り当てで壊れる問題を避けるため。
  - **OS が mDNS で名乗っている名前を案内し、WebLAV 自身は名乗らない** (`src/mdns.rs`)。
    macOS (mDNSResponder)・Windows・Linux (Avahi) は、すでに PC の名前で `<名前>.local` に応えている。
    WebLAV が同じ名前を重ねて名乗ると、OS は LAN の別の機器との衝突とみなして PC の名前を付け直す
    (macOS で、起動のたびに「ローカルホスト名」の末尾の数字が進むのを実測した)。
    - macOS は「共有」のローカルホスト名 (LocalHostName) を読む。`gethostname` は DHCP で配られた名前を返すことがあり、Bonjour の名前と一致するとはかぎらない。
    - Windows と Linux は、コンピュータ名 (`gethostname`) のドメイン部分を除いたものを使う。英数字とハイフン以外が混じるときは案内しない。別の名前に直すと、誰も応えない名前を案内することになるため。
      Windows 11 と Ubuntu 26.04 の Avahi で、この名前が LAN の他の端末から引けるのを実測した (大文字小文字は区別されない)。
    - macOS は衝突で名前を付け直すことがあるので、起動時に決めず、案内するたびに読む。Linux の Avahi が付け直した名前 (`host-2.local`) や、`avahi-daemon.conf` の `host-name` で変えた名前は `gethostname` に現れず、追えない。
    - 案内するのは、全インターフェースで待ち受けている (`bind = "0.0.0.0"`) ときだけ。OS の mDNS は全インターフェースのアドレスで応えるので、特定のアドレスへの bind では開けない URL を案内しうる。
  - **`GET /api/v1/connection-info` が現在のホスト名とポートを返す**。認証は掛けない
    (同じ LAN にいなければ到達できないアドレスであり秘密ではないため)。
    名前を読めない・案内しない bind のときは `mdnsHostname` が `null` になる。
  - **共通ヘッダーの「接続」アイコンから、QRコードとURL (コピーボタン付き) を表示するダイアログを開く**。
    ログインの有無を問わず誰でも開ける。LAN 内にいる時点で到達できる情報であり、誰に見せても実害は無いため。

### 接続ダイアログの見た目

- URL の表示は本文と同じ書体 (BIZ UDPGothic) にする。等幅書体はマニュアルのコード表示専用
  (→ ui.md「UI 全般」) であり、URL は読者が確認・入力する対象であって code 片ではないため。
- URL は省略せず、折り返して全文を見せる。手入力したい人にも対応するため。
  コピーボタンでの利用を主に想定しても、全文確認の手段を削らない。
- QRコード自体の白地・quiet zone は変えないまま、白背景・角丸・薄い枠のカードに載せる。
  ダークモードで白い画像領域だけが唐突に現れる印象を和らげる。

### タスクトレイの表示言語

- **メニュー (`ブラウザーで開く`/`Quit` に相当する項目) は OS の表示言語に合わせる**。日本語の OS なら
  日本語、それ以外は英語にする (二択、フロントエンドの言語トグルとは独立)。
  共有フォルダーの設定より前に、使うのが難しい人が最初に触るのがトレイのため。
  `src/tray/mod.rs` が判定する。Windows は `GetUserDefaultUILanguage`、macOS は OS の優先する言語の一覧
  (`NSLocale.preferredLanguages`) の先頭、Linux は POSIX のロケール環境変数
  (`LC_ALL` → `LC_MESSAGES` → `LANG` の優先順) を見る
  (→ AGENTS.md「バックエンドが出す文字列は英語で統一する」の例外)。
  macOS で環境変数を見ないのは、ログイン項目や Finder から起動すると `LANG` が渡らないため。
- ツールチップの内容 (アプリ名 `weblav` のみ、→ 「LAN からの到達性」) は言語で変わらない。

## データモデルの全体像

コンテンツは5種類 (`link` / `file` / `folder` / `group` / `archive`) を
`contents` テーブル1本で表す。type ごとに必要なカラムだけを埋め、
「type ごとにどれが必須か」はアプリケーション層 (Rust) で検証する
(既存の `url` / `path` / `blob_hash` と同じ方針)。

`folder` / `archive` も `group` の配下に置ける (`parent_id` を持てる)。
親になれるのは `group` だけ。
`folder` を `authenticated` のグループ配下に置いて、中身ごとログインした人だけに見せられるようにするため。

`archive` 固有の情報だけは行数が増えるため別テーブルに分ける。

| テーブル | 役割 |
|---|---|
| `contents` | 5種類のコンテンツ。`archive` は `path` + `extensions` + `title_template` を使う |
| `archive_items` | アーカイブ配下のファイル1件。相対パスと公開フラグのみ |
| `archive_axes` | 属性軸の定義 (名前・抽出元・階層番号) |
| `archive_axis_values` | 軸の値の辞書 (生の値・表示名・並び順) |

**アイテムごとの軸の値は保存しない**。`archive_items.rel_path` から読み出し時に
毎回導出する (→ archive.md「軸の値の辞書と導出」)。理由は3つ。

- 軸の定義や照合語リストを変えた直後から、再スキャンを待たずに結果へ反映される。
  「語を足したのに反映されない」という事故が原理的に起きない。
- アイテム×軸の中間テーブルが不要になり、その同期処理も要らない。
- 想定する規模が、索引するファイル数千件×軸10本程度であり、導出コストが問題にならない。

## データモデル

### 既存テーブルの変更

`contents` の `type` と `visibility` には `CHECK` 制約を置かない。
SQLite では `CHECK` を後から変えられずテーブル再作成が要るのに対し、
type ごとの必須カラムの検証 (`url` / `path` / `blob_hash`) は結局
アプリケーション層が担うため、列挙値だけ DB 側に二重に持つ意味が薄い。

**WebLAV はまだ配布していないため、移行すべき既存データが存在しない**。
そのため `CHECK` 制約は最初の `create_contents` マイグレーションから外し、
テーブルを作り直すマイグレーションは書かない。
`archive` 用の2カラム (`extensions` / `title_template`) は
`ALTER TABLE ADD COLUMN` で足りる。

`private` のための `created_by` は、`ALTER TABLE ADD COLUMN` の新しいマイグレーションで足す。
既存のマイグレーションは他のワークツリーへ適用済みのため書き換えない (下記)。
既存の行は `created_by` が NULL (作成者なし) になる。

コンテンツは表示順 (`position`) を持たない (→ ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。
既存の列は、索引を作り直してから `ALTER TABLE DROP COLUMN` する新しいマイグレーションで落とす。

**一度でもマイグレーションを他の環境へ共有・適用した時点で、この判断は使えなくなる**。
境界は配布物を出す時点ではない。別の開発機や別ワークツリーに渡って適用された
時点で同じ制約がかかる。それ以降は適用済みのマイグレーションファイルを編集も改名も
せず、変更は必ず新しいファイルとして足す。既存データを保つ書き方に切り替える。

なお開発中は、マイグレーションを書き換えるたびに手元の DB を作り直す必要がある
(`DEVELOPMENT.md`「注意すること」)。sqlx がチェックサムを照合するため、
中身が空でも履歴が残っていれば起動しない。

### スキーマ

```sql
-- contents (CHECK制約なし。archive用の2カラムと created_by は ALTER TABLE で追加する)
CREATE TABLE contents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 'link' | 'file' | 'folder' | 'group' | 'archive'
    -- 値の検証はアプリケーション層で行う (→ 「既存テーブルの変更」)
    type TEXT NOT NULL,
    parent_id INTEGER REFERENCES contents (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    url TEXT,          -- link のみ必須
    path TEXT,         -- folder / archive のみ必須。canonicalize済みの絶対パス
    description TEXT,
    -- 'public' | 'authenticated' | 'private' | 'hidden'。
    -- 親グループから継承する (→ access.md「祖先のグループを辿る」・「親が外れるときの公開範囲」)。group は private にできない
    visibility TEXT NOT NULL DEFAULT 'authenticated',
    -- 作成したユーザー。NULL は作成者なし。private と書き換えの判定に使う (→ access.md「公開範囲」・「ロールと操作」)。
    -- ON DELETE は指定しない。ユーザーの削除は access.md「ユーザーの削除と作成者」の手順を必ず通す
    created_by INTEGER REFERENCES users (id),
    blob_hash TEXT,    -- file のみ。SHA-256(64桁16進)。実体は blobs_dir/<hash>
    file_name TEXT,    -- file のみ
    file_size INTEGER, -- file のみ
    -- archive のみ。索引対象の拡張子をカンマ区切りで保持する。NULL/空なら全ファイル
    extensions TEXT,
    -- archive のみ。表示タイトルのテンプレート (例: '{科目}（{資料種別}）')。
    -- NULL ならファイル名をそのまま表示する
    title_template TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_contents_blob_hash ON contents (blob_hash);
CREATE INDEX idx_contents_parent_id ON contents (parent_id, id);
CREATE INDEX idx_contents_created_by ON contents (created_by);

-- アーカイブ配下のファイル1件。属性の値はここに持たず、rel_path から
-- 読み出し時に導出する (→ archive.md「軸の値の辞書と導出」)。
CREATE TABLE archive_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    archive_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    -- アーカイブの登録先フォルダーからの相対パス。区切りは '/' に正規化して保存する。
    -- 同一性の判断はこの値のみで行う。
    rel_path TEXT NOT NULL,
    -- 0=非公開。スキャンで新しく見つかった行は必ず 0 から始まる。
    published INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (archive_id, rel_path)
);

-- 一覧は「このアーカイブの公開アイテム」を全件引くため。
CREATE INDEX idx_archive_items_archive_id ON archive_items (archive_id, published);

-- 属性軸の定義。
CREATE TABLE archive_axes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    archive_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    -- 軸の表示名。title_template のプレースホルダーはこの名前で参照する。
    -- リネーム時は同一トランザクションで title_template も書き換える。
    name TEXT NOT NULL,
    -- 'dir_level' | 'filename_word'
    source TEXT NOT NULL,
    -- source='dir_level' のときのみ使う。アーカイブ登録先の直下を 1 とする階層番号。
    dir_level INTEGER,
    -- 1=閲覧側の絞り込みに出す。0 でも表示タイトルとアイテムの並び順には使う(→ archive.md「軸の定義」)。
    filterable INTEGER NOT NULL DEFAULT 1,
    -- 1=値が無くても表示タイトルを組み立てる(未設定の部分を空にする → archive.md「表示タイトル」)。
    optional_in_title INTEGER NOT NULL DEFAULT 0,
    -- 軸そのものの並び順(絞り込みUIでの左からの順番)。
    position INTEGER NOT NULL DEFAULT 0,
    UNIQUE (archive_id, name)
);

CREATE INDEX idx_archive_axes_archive_id ON archive_axes (archive_id, position, id);

-- 軸の値の辞書。source によって意味が変わる。軸の source か dir_level を変えると行を消す(→ archive.md「軸の値の辞書と導出」)。
--   dir_level    : スキャン時に自動収集された値。display_name は管理者が後から付ける
--   filename_word: この行そのものが照合語リスト。raw_value が照合する語。
--                  照合は最長一致。同じ長さの語はリストの上(position が小さい方)を採る
CREATE TABLE archive_axis_values (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    axis_id INTEGER NOT NULL REFERENCES archive_axes (id) ON DELETE CASCADE,
    raw_value TEXT NOT NULL,
    -- NULL または空文字なら raw_value をそのまま表示する(→ archive.md「軸の値の辞書と導出」)。
    display_name TEXT,
    -- 表示順(→ archive.md「軸の値の辞書と導出」)。
    position INTEGER NOT NULL DEFAULT 0,
    -- 'anywhere' | 'word_start' | 'end'。filename_word 軸の行だけが anywhere 以外を持てる(→ archive.md「軸の定義」)。
    match_position TEXT NOT NULL DEFAULT 'anywhere',
    UNIQUE (axis_id, raw_value)
);

CREATE INDEX idx_archive_axis_values_axis_id
    ON archive_axis_values (axis_id, position, id);

-- サイト全体の設定。1行だけ (id = 1)。未設定は空文字列 (→ ui.md「UI 全般」)。
-- 項目ごとに型と既定値を持たせるため、キーと値の表にしない。
CREATE TABLE site_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    site_name TEXT NOT NULL DEFAULT '',
    home_heading TEXT NOT NULL DEFAULT ''
);

-- 公開できるフォルダー (→ folders.md「公開できるフォルダー」)。path は canonicalize 済みの絶対パス。
-- 削除しても行は残し deleted_at を付ける (残ったコンテンツの場所を、起点を隠して出すため)。
CREATE TABLE roots (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    deleted_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE UNIQUE INDEX roots_name_nocase ON roots (name COLLATE NOCASE) WHERE deleted_at IS NULL;

-- macOS のサンドボックスで、窓で選んだフォルダーを次の起動でも読むためのブックマーク (→ distribution.md「ビルド・配布の方法」)。
-- 窓で選んだ時点 (登録の前) に書くので roots と分ける。path は canonicalize 済みの絶対パス。
CREATE TABLE folder_bookmarks (
    path TEXT PRIMARY KEY NOT NULL,
    bookmark BLOB NOT NULL
);

-- リンクのカードに出すページの情報のキャッシュ (→ ui.md「リンクのカード」)。URL ごとに1行。
-- 画像の実体はデータの置き場の link-previews/ に置く。バックアップから戻すときは戻さない。
CREATE TABLE link_previews (
    url TEXT PRIMARY KEY NOT NULL,
    title TEXT, site_name TEXT, published_at TEXT,
    image_source TEXT, image_file TEXT, icon_source TEXT, icon_file TEXT,
    etag TEXT, last_modified TEXT,
    fresh_until INTEGER NOT NULL DEFAULT 0,  -- unix 秒。ここまでは確かめずに使う
    checked_at INTEGER NOT NULL DEFAULT 0,   -- 最後に確かめに行った時刻
    shown_at INTEGER NOT NULL DEFAULT 0,     -- 最後に一覧に出た時刻。上限を超えたら古い順に消す
    bytes INTEGER NOT NULL DEFAULT 0
);
```

`users` テーブルは変更しない (`id` / `username` / `password_hash` / `role` /
`created_at`)。セッション用テーブルは `tower-sessions-sqlx-store` が別途作成する。
