-- AI が集めるリンクで、検索で引くページの新しさをグループごとに選べるようにする (→ docs/ai.md「AI が集めるリンク」)。
-- 話題になっているものを集めたいグループでは、1年以内では古い記事や定番のページが多く混じるため。
-- 'any' は、絞らない検索と1年以内の検索を1回ずつ引く (今までの動き)。'year'・'month'・'week'・'day' は、その長さに絞った1回だけ。
ALTER TABLE ai_link_groups ADD COLUMN period TEXT NOT NULL DEFAULT 'any';
