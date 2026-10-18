-- AI が集めるリンクで、検索キーワードの行を書いたとおりのクエリにする (→ docs/ai.md「AI が集めるリンク」)。
-- AI に作らせたクエリでは、結果を見て利用者がクエリを直せないため。作って残していたクエリと、その言語は使わなくなる。
-- 今の検索キーワードは、そのまま1つ (行があれば行ごと) のクエリとして使う。
ALTER TABLE ai_link_groups DROP COLUMN queries;
ALTER TABLE ai_link_groups DROP COLUMN queries_language;
