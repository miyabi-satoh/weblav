-- WebLAV の DB の形 (→ docs/architecture.md「データモデル」)。
--
-- 型ごとの必須の列 (link の url、folder と shortcut の path など) と、列挙の値 (contents.type・visibility ほか) は
-- Rust の型で確かめ、CHECK を付けない。SQLite は CHECK を後から変えられず、値を足すたびにテーブルを作り直すことになるため。
-- セッションの表は tower-sessions-sqlx-store が別に作る。

CREATE TABLE users (
    -- AUTOINCREMENT: 消したユーザーの id を使い回すと、残っていたセッションが新しいユーザーとして通るため。
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    role TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    -- リカバリコード (→ docs/access.md「リカバリコード」)。作った時刻は、手元の控えが今使えるものかを見比べるために出す。
    recovery_code_hash TEXT,
    recovery_code_created_at TEXT
);

-- ログインの名前は大文字と小文字を区別しない。
CREATE UNIQUE INDEX users_username_nocase ON users (username COLLATE NOCASE);

CREATE TABLE contents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 'link' | 'folder' | 'group' | 'shortcut'
    type TEXT NOT NULL,
    -- グループを消すときは、直接の子をトップへ移してから消す (`api::contents::delete_content`)。
    -- CASCADE は、その前提が崩れたときの最後の守り。
    parent_id INTEGER REFERENCES contents (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    -- link のみ必須。
    url TEXT,
    description TEXT,
    -- 'public' | 'authenticated' | 'private' | 'hidden'。
    -- 親のグループから継承する (→ docs/access.md「祖先のグループを辿る」・「親が外れるときの公開範囲」)。
    visibility TEXT NOT NULL DEFAULT 'authenticated',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- folder と shortcut で必須。canonicalize 済みの絶対パス。
    path TEXT,
    -- folder のみ。対象と除外のパターン (→ docs/folders.md「見せるものの絞り込み」)。1行に1つの glob。NULL なら絞らない。
    include_patterns TEXT,
    -- folder の一覧の表示のみ。表示タイトルのテンプレート (例: '{科目}（{資料種別}）')。NULL ならファイル名をそのまま出す。
    title_template TEXT,
    -- 作ったユーザー。NULL は作成者なし。private と書き換えの判定に使う (→ docs/access.md「公開範囲」・「ロールと操作」)。
    -- ON DELETE は付けない。ユーザーの削除は docs/access.md「ユーザーの削除と作成者」の手順を通す。
    created_by INTEGER REFERENCES users (id),
    -- folder のみ。一覧の表示 (索引して軸で絞り込む) を使うか。
    list_view INTEGER NOT NULL DEFAULT 0,
    -- 最後にスキャンし終えた時刻 (Unix ミリ秒)。自動の再スキャンの次の回をここから数える。
    last_scanned_at INTEGER,
    -- 自動の再スキャン。'off'・'interval' (前回から auto_rescan_hours 時間たったら)・'daily' (毎日 auto_rescan_time を過ぎたら)。
    -- auto_rescan_time は動かす PC の現地時刻の 'HH:MM'。要る値がそろっているかは保存する側 (`api::schedule::parse_auto_refresh`) で確かめる。
    auto_rescan_mode TEXT NOT NULL DEFAULT 'off',
    auto_rescan_hours INTEGER,
    auto_rescan_time TEXT,
    -- hidden にする前の公開範囲。「表示に戻す」で元に戻すため (→ docs/access.md「非表示のコンテンツ」)。
    -- 公開範囲を変える箇所ごとに書くと書き漏らすので、下のトリガーの1か所で書く。
    visibility_before_hidden TEXT,
    -- folder のみ。除外のパターン。形は include_patterns と同じ。
    exclude_patterns TEXT
);

CREATE INDEX idx_contents_created_by ON contents (created_by);
-- 直下の一覧 (WHERE parent_id = ?) とトップの一覧 (WHERE parent_id IS NULL) の両方に使う。
CREATE INDEX idx_contents_parent_id ON contents (parent_id, id);

CREATE TRIGGER contents_remember_visibility_before_hidden
AFTER UPDATE OF visibility ON contents
WHEN NEW.visibility = 'hidden' AND OLD.visibility != 'hidden'
BEGIN
    UPDATE contents SET visibility_before_hidden = OLD.visibility WHERE id = NEW.id;
END;

CREATE TRIGGER contents_forget_visibility_before_hidden
AFTER UPDATE OF visibility ON contents
WHEN NEW.visibility != 'hidden' AND OLD.visibility = 'hidden'
BEGIN
    UPDATE contents SET visibility_before_hidden = NULL WHERE id = NEW.id;
END;

-- 一覧の表示を使うフォルダーの、索引したファイル1件。軸の値はここに持たず、rel_path から読むときに導出する
-- (→ docs/archive.md「軸の値の辞書と導出」)。
CREATE TABLE archive_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    archive_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    -- 登録先のフォルダーからの相対パス。区切りは '/' にそろえる (Windows とほかの OS で同じ値にするため)。
    -- 同じアイテムかはこの値だけで決める。
    rel_path TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- スキャンで読んだプレビューの情報 (→ docs/archive.md「スキャン」)。一覧のたびにファイルを開くと、NAS の数万枚の写真で十数分かかるため。
    -- file_size と modified_ns は、変わったファイルだけを読み直す目印。file_size が NULL ならまだ読んでいない。
    -- 画像でない・大きさを読めないファイルは image_width・image_height が NULL。
    file_size INTEGER,
    modified_ns INTEGER,
    image_width INTEGER,
    image_height INTEGER,
    is_text INTEGER NOT NULL DEFAULT 0,
    -- 写真の撮影日 (EXIF、Unix 秒)。日付の軸が使う。無ければ更新日 (modified_ns) に落とす。
    taken_at INTEGER,
    -- どの版の読み方で読んだか。読み方を変えたら上げ、古い版で読んだ行を、変わっていなくても読み直す。
    read_version INTEGER NOT NULL DEFAULT 1,
    UNIQUE (archive_id, rel_path)
);

CREATE INDEX idx_archive_items_archive_id ON archive_items (archive_id);

-- 索引したフォルダーとその更新日時。一覧の表示を開いたときに、変わったフォルダーだけを読み直す手掛かり
-- (フォルダーは、中にファイルを足したり消したりすると更新日時が変わる)。
CREATE TABLE archive_dirs (
    archive_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    rel_path TEXT NOT NULL,
    modified_ns INTEGER,
    PRIMARY KEY (archive_id, rel_path)
);

-- 軸の定義。
CREATE TABLE archive_axes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    archive_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    -- 軸の表示名。title_template のプレースホルダーはこの名前で指すので、名前を変えるときは同じトランザクションで
    -- title_template も書き換える。'{' '}' を含めないことは Rust で確かめる。
    name TEXT NOT NULL,
    -- 'dir_level' (フォルダーの第N階層) | 'filename_word' (ファイル名に含まれる語) | 'date_year' | 'date_month'
    source TEXT NOT NULL,
    -- source = 'dir_level' のときだけ使う。登録先の直下を 1 とする階層の番号。
    dir_level INTEGER,
    -- 軸そのものの並び順 (絞り込みで左からの順番)。
    position INTEGER NOT NULL DEFAULT 0,
    -- 1 なら閲覧側の絞り込みに出す。0 でも表示タイトルとアイテムの並び順には使う (→ docs/archive.md「軸の定義」)。
    filterable INTEGER NOT NULL DEFAULT 1,
    -- 1 なら値が無くても表示タイトルを組み立てる (→ docs/archive.md「表示タイトル」)。
    optional_in_title INTEGER NOT NULL DEFAULT 0,
    UNIQUE (archive_id, name)
);

CREATE INDEX idx_archive_axes_archive_id ON archive_axes (archive_id, position, id);

-- 軸の値の辞書 (→ docs/archive.md「軸の値の辞書と導出」)。
--   dir_level    : スキャンで集めた値。display_name は管理者が後から付ける。
--   filename_word: この行そのものが照合する語のリスト。
CREATE TABLE archive_axis_values (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    axis_id INTEGER NOT NULL REFERENCES archive_axes (id) ON DELETE CASCADE,
    raw_value TEXT NOT NULL,
    -- NULL なら raw_value をそのまま出す。
    display_name TEXT,
    -- 表示の順と照合の優先を兼ねる (並べ替えを2つに増やさないため)。
    position INTEGER NOT NULL DEFAULT 0,
    -- 'anywhere' | 'word_start' | 'end'。filename_word の軸の行だけが anywhere 以外を持てる (→ docs/archive.md「軸の定義」)。
    match_position TEXT NOT NULL DEFAULT 'anywhere',
    UNIQUE (axis_id, raw_value)
);

CREATE INDEX idx_archive_axis_values_axis_id
    ON archive_axis_values (axis_id, position, id);

-- 一覧の表示を使うフォルダーごとの、索引と導出の材料 (index_version) と、公開の判定の材料 (filter_version) の版。
-- 導出の結果と判定をサーバーのメモリに持つので、変わったら作り直すため (→ docs/archive.md「導出の結果をサーバーのメモリに持つ」)。
-- 版はトリガーで進める。書き換える経路 (スキャン・開いたときの読み直し・軸・辞書・テンプレート・設定) が多く、
-- コードの側で進めると1つ漏らしただけで古い一覧や古い公開を返すため。
-- 外部キーは張らない。コンテンツを消したときに連鎖で消える索引の行のトリガーが、消している最中のコンテンツを指す行を
-- 足そうとして外部キーの検査に掛かるため。残った行は、contents.id を使い回さない (AUTOINCREMENT) ので害が無い。
CREATE TABLE archive_versions (
    content_id INTEGER PRIMARY KEY,
    index_version INTEGER NOT NULL DEFAULT 0,
    filter_version INTEGER NOT NULL DEFAULT 0
);

CREATE TRIGGER archive_items_bump_insert AFTER INSERT ON archive_items BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_items_bump_update AFTER UPDATE ON archive_items BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_items_bump_delete AFTER DELETE ON archive_items BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (OLD.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;

CREATE TRIGGER archive_axes_bump_insert AFTER INSERT ON archive_axes BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_axes_bump_update AFTER UPDATE ON archive_axes BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_axes_bump_delete AFTER DELETE ON archive_axes BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (OLD.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;

-- 値の辞書は、軸を通してフォルダーを引く。
CREATE TRIGGER archive_axis_values_bump_insert AFTER INSERT ON archive_axis_values BEGIN
    INSERT INTO archive_versions (content_id, index_version)
    SELECT archive_id, 1 FROM archive_axes WHERE id = NEW.axis_id
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_axis_values_bump_update AFTER UPDATE ON archive_axis_values BEGIN
    INSERT INTO archive_versions (content_id, index_version)
    SELECT archive_id, 1 FROM archive_axes WHERE id = NEW.axis_id
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_axis_values_bump_delete AFTER DELETE ON archive_axis_values BEGIN
    INSERT INTO archive_versions (content_id, index_version)
    SELECT archive_id, 1 FROM archive_axes WHERE id = OLD.axis_id
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;

-- コンテンツの設定のうち、導出の材料 (表示タイトルのテンプレート・登録先) と、公開の判定の材料 (対象と除外のパターン)。
CREATE TRIGGER contents_bump_index AFTER UPDATE OF title_template, path ON contents BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER contents_bump_filter AFTER UPDATE OF include_patterns, exclude_patterns ON contents BEGIN
    INSERT INTO archive_versions (content_id, filter_version) VALUES (NEW.id, 1)
    ON CONFLICT (content_id) DO UPDATE SET filter_version = filter_version + 1;
END;

-- サイト全体の設定。1行だけ (id = 1)。項目ごとに型と既定値を持たせるため、キーと値の表にしない。
-- 未設定は空文字列で、画面は空の項目を出さない。
CREATE TABLE site_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    site_name TEXT NOT NULL DEFAULT '',
    home_heading TEXT NOT NULL DEFAULT ''
);

INSERT INTO site_settings (id) VALUES (1);

-- 共有 (→ docs/shares.md)。path は canonicalize 済みの絶対パス。
CREATE TABLE roots (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    name TEXT NOT NULL DEFAULT '',
    -- 消しても行は残して印を付ける (残ったコンテンツの場所を、起点を隠して出すため)。
    deleted_at TEXT,
    -- 一般ユーザーの既定のできること。'none' | 'read' | 'add' | 'all' (→ docs/shares.md「できること」)。
    -- 管理者は常にすべてできるので持たない。
    editor_access TEXT NOT NULL DEFAULT 'read'
);

-- 名前は、消していない共有の中で大文字と小文字を区別せずに重ならない。
CREATE UNIQUE INDEX roots_name_nocase ON roots (name COLLATE NOCASE) WHERE deleted_at IS NULL;

-- 共有ごとの、ユーザーごとの例外。行があれば roots.editor_access の代わりに使う。
-- 共有は消しても行を残すので、登録し直すと例外も戻る。
CREATE TABLE root_user_access (
    root_id INTEGER NOT NULL REFERENCES roots (id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    access TEXT NOT NULL,
    PRIMARY KEY (root_id, user_id)
);

-- 共有の中を書き換えた操作の記録 (→ docs/shares.md「操作の記録」)。ユーザーや共有を消しても残すため、外部キーを張らず名前を写す。
CREATE TABLE share_operations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    user_id INTEGER,
    username TEXT NOT NULL,
    -- 'upload' | 'mkdir' | 'rename' | 'move' | 'copy' | 'delete' | 'list_on_home'
    op TEXT NOT NULL,
    root_id INTEGER NOT NULL,
    root_name TEXT NOT NULL,
    -- 扱った場所の、共有からの相対パス ('/' 区切り) の JSON の配列。いくつかを選んで動かした・消したときは、
    -- 選んだものを並べる (中のファイルまでは並べない)。
    paths TEXT NOT NULL,
    -- 移動・コピーの置き先のフォルダーと、名前の変更の後の場所。
    dest_root_id INTEGER,
    dest_root_name TEXT,
    dest_path TEXT,
    -- 扱ったファイル・フォルダーの数と、ファイルの大きさの合計。終わるまで分からないものは NULL。
    item_count INTEGER,
    byte_count INTEGER,
    -- 'running' | 'succeeded' | 'failed' | 'cancelled'
    result TEXT NOT NULL,
    -- 失敗したときの API のエラーの code (`error::ErrorCode`)。
    error TEXT
);

CREATE INDEX idx_share_operations_at ON share_operations (at);

-- macOS のサンドボックスで、窓で選んだフォルダーを次の起動でも読むためのブックマーク (→ docs/distribution.md「ビルド・配布の方法」)。
-- 窓で選んだ時点 (登録の前) に書くので roots と分ける。path は canonicalize 済みの絶対パス。
CREATE TABLE folder_bookmarks (
    path TEXT PRIMARY KEY NOT NULL,
    bookmark BLOB NOT NULL,
    -- 1 なら読み書きの許可、0 なら読むだけの許可 (→ docs/shares.md「macOS の書き込みの許可」)。
    read_write INTEGER NOT NULL DEFAULT 0
);

-- リンクのカードに出すページの情報のキャッシュ (→ docs/ui.md「リンクのカード」)。URL ごとに1行。
-- 画像の実体はデータの置き場の link-previews/ に置く。INTEGER の時刻 (fresh_until・checked_at・shown_at) は Unix 秒。
CREATE TABLE link_previews (
    url TEXT PRIMARY KEY NOT NULL,
    title TEXT,
    site_name TEXT,
    -- ページを最後に更新した日時 (無ければ公開した日時)。文字列 (→ `api::link_preview::LinkPreview::updated_at`)。
    updated_at TEXT,
    -- 画像とアイコンを取った元の URL。ページの指定が変わったときだけ取り直す。
    image_source TEXT,
    image_file TEXT,
    icon_source TEXT,
    icon_file TEXT,
    -- 条件付きで取り直すための、前回の応答の検証子。
    etag TEXT,
    last_modified TEXT,
    -- この時刻までは、確かめに行かずに使う。
    fresh_until INTEGER NOT NULL DEFAULT 0,
    -- 最後に確かめに行った時刻。取れなかったときも進め、相手を何度も突かないようにする。
    checked_at INTEGER NOT NULL DEFAULT 0,
    -- 最後に一覧に出た時刻。全体の上限を超えたら、古い順に消す。
    shown_at INTEGER NOT NULL DEFAULT 0,
    -- この行の画像とアイコンのバイト数。全体の上限に数える。
    bytes INTEGER NOT NULL DEFAULT 0,
    description TEXT,
    -- 最後に確かめたとき、ページが無くなっていた (404・410) か。
    gone INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX link_previews_shown_at ON link_previews (shown_at);
