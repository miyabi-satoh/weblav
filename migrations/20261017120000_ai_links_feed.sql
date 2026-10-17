-- AI が集めるリンクを、Discover のように「更新」で足していく形にする (→ docs/ai.md「AI が集めるリンク」)。
-- AI の選別をやめ、検索の結果をそのまま並べる。合わないものは人が「表示しない」で外す。
--
-- ai_link_groups に足す列:
-- - queries: AI が検索キーワードから作ったクエリ (JSON の文字列の配列)。NULL なら次に集めるときに作る。
--   更新のたびに作り直すと、結果の変化が Web の変化かクエリの変化か分からなくなるため、作ったものを使い回す。
--   検索キーワードを変えたときに NULL に戻す。queries_language は作ったときの表示言語。
-- - daily_limit: 1日に集め直せる回数。0 は無制限。
-- - auto_mode: 自動の更新。'off'・'interval' (前回から auto_hours 時間たったら)・'daily' (毎日 auto_time を過ぎたら)。
--   auto_time は PC の現地時刻の 'HH:MM'。値の組み合わせは src/api/ai_links.rs で検証する。
-- - refreshed_by_auto: 最後の集め直しが自動の更新か。
-- - refresh_day・refresh_count: 現地時刻の日付 ('YYYY-MM-DD') と、その日に集め直した回数。日付が変わったら 0 から数える。
ALTER TABLE ai_link_groups ADD COLUMN queries TEXT;
ALTER TABLE ai_link_groups ADD COLUMN queries_language TEXT;
ALTER TABLE ai_link_groups ADD COLUMN daily_limit INTEGER NOT NULL DEFAULT 0;
ALTER TABLE ai_link_groups ADD COLUMN auto_mode TEXT NOT NULL DEFAULT 'off';
ALTER TABLE ai_link_groups ADD COLUMN auto_hours INTEGER;
ALTER TABLE ai_link_groups ADD COLUMN auto_time TEXT;
ALTER TABLE ai_link_groups ADD COLUMN refreshed_by_auto INTEGER NOT NULL DEFAULT 0;
ALTER TABLE ai_link_groups ADD COLUMN refresh_day TEXT;
ALTER TABLE ai_link_groups ADD COLUMN refresh_count INTEGER NOT NULL DEFAULT 0;

-- 集めたリンクは、入れ替えでなく足していく。並べるのは、最後の検索の結果に出ているものだけ。
-- 並びは、見つけた時刻 (found_at) の新しい順、同じ時刻の中は日付 (date) の新しい順、日付が無ければ検索の順 (rank)。
-- 位置の列で並びを持つのをやめるので、表を作り直す。今あるリンクは、最後に集めた時刻に見つけたものとして写す。
CREATE TABLE ai_links_new (
    content_id INTEGER NOT NULL REFERENCES ai_link_groups (content_id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    date TEXT,
    rank INTEGER NOT NULL,
    found_at INTEGER NOT NULL,
    PRIMARY KEY (content_id, url)
);
INSERT INTO ai_links_new (content_id, url, title, date, rank, found_at)
SELECT ai_links.content_id, ai_links.url, ai_links.title, NULL, ai_links.position,
       COALESCE(ai_link_groups.refreshed_at, 0)
FROM ai_links JOIN ai_link_groups ON ai_link_groups.content_id = ai_links.content_id;
DROP TABLE ai_links;
ALTER TABLE ai_links_new RENAME TO ai_links;

-- 「表示しない」にしたページ (kind = 'page'、value は URL) とサイト (kind = 'site'、value はホスト名)。
-- 一覧から取り消せるよう、表示しないにしたときのタイトルを残す。グループの設定を外すと一緒に消える。
CREATE TABLE ai_link_exclusions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content_id INTEGER NOT NULL REFERENCES ai_link_groups (content_id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (content_id, kind, value)
);
