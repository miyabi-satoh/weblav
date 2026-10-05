-- リンクのカードに出す、ページの情報のキャッシュ (→ docs/ui.md「リンクのカード」)。
--
-- URL ごとに1行。一覧に出たときに取りに行き、HTTP のキャッシュの決まり (RFC 9111) で取り直す時期を決める。
-- 画像の実体はバックアップに入らないので、バックアップから戻すときはこの表を戻さずに今の行を残す (→ api::backup)。
-- 画像とアイコンの実体はデータの置き場の `link-previews/` に置き、ここにはファイル名だけを持つ。
-- 時刻はどれも unix 秒。比べるだけで画面には出さないため。
CREATE TABLE link_previews (
    url TEXT PRIMARY KEY NOT NULL,
    title TEXT,
    site_name TEXT,
    published_at TEXT,
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
    bytes INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX link_previews_shown_at ON link_previews (shown_at);
