-- AI が集めるリンクで、検索で見つかった一覧のページ (分類のトップなど) より個別の記事を先に並べる (→ docs/ai.md「一覧のページ」)。
-- 検索の業者は広いクエリに分類のトップを上位で返すので、そのまま並べると個別の記事が埋もれるため。
--
-- - ai_link_groups.list_mode: 一覧のページに載っている記事の使い方。'off' (開かない)・'pick' (AI が合うものを選ぶ)・
--   'search' (その話題でもう一度検索する)。どれが良いかを実際の結果で比べるため、グループごとに選ばせる。
-- - ai_links.is_list: 一覧のページか。URL の形で見分け (src/api/ai_link_pages.rs)、同じときに見つけたものの中で後ろに並べる。
--   今ある行は、次に集め直すまで 0 のままにする。
ALTER TABLE ai_link_groups ADD COLUMN list_mode TEXT NOT NULL DEFAULT 'pick';
ALTER TABLE ai_links ADD COLUMN is_list INTEGER NOT NULL DEFAULT 0;
