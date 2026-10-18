-- AI が集めるリンクで、検索で引くページの新しさを、決まった選択肢でなく日数で決める (→ docs/ai.md「AI が集めるリンク」)。
-- 3日や10日のような間の長さも選べるようにするため。業者には公開日の下限 (この日より後) として渡す。
-- within_days: 公開から何日以内に絞るか。0 は絞らない。今の選択肢は同じ長さの日数に移す ('any' は 0)。
ALTER TABLE ai_link_groups ADD COLUMN within_days INTEGER NOT NULL DEFAULT 0;
UPDATE ai_link_groups SET within_days = CASE period
    WHEN 'year' THEN 365
    WHEN 'month' THEN 30
    WHEN 'week' THEN 7
    WHEN 'day' THEN 1
    ELSE 0
END;
ALTER TABLE ai_link_groups DROP COLUMN period;
