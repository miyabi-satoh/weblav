-- AI が集めるリンク (→ docs/ai.md「AI が集めるリンク」)。グループに付ける機能で、付けたグループごとに1行。
--
-- contents に列を足さず別の表にする。コンテンツの作成・更新 (完全上書きの PUT) と切り離して、
-- 付け外しと関心の言葉だけを書き換えられるようにするため。
-- content_id が group を指すことと、interests の形 (行の数・長さ) は src/api/ai_links.rs で検証する。
-- refreshed_at は最後に集めた時刻 (UNIX 秒)。まだ集めていなければ NULL。
CREATE TABLE ai_link_groups (
    content_id INTEGER PRIMARY KEY REFERENCES contents (id) ON DELETE CASCADE,
    interests TEXT NOT NULL,
    refreshed_at INTEGER
);

-- 最後に集めた結果。集め直すたびに、そのグループの分を入れ替える。
-- 持つのは URL とタイトルだけ。カードの説明と画像は、ほかのリンクと同じく link_previews から出す。
CREATE TABLE ai_links (
    content_id INTEGER NOT NULL REFERENCES ai_link_groups (content_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    PRIMARY KEY (content_id, position)
);
