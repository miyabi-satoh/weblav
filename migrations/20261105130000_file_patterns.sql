-- 「対象の拡張子」を、対象と除外のパターン (1行に1つの glob) にする (→ docs/folders.md「公開と非公開」)。
-- 拡張子だけでなく名前やフォルダーでも絞れ、除外もできるようにするため。
ALTER TABLE contents RENAME COLUMN extensions TO include_patterns;
ALTER TABLE contents ADD COLUMN exclude_patterns TEXT;

-- 保存してあるのは `mp3,pdf` の形なので、`*.mp3` と `*.pdf` の2行に直す。
UPDATE contents
SET include_patterns = '*.' || replace(include_patterns, ',', char(10) || '*.')
WHERE include_patterns IS NOT NULL;

-- 除外も公開の判定の材料なので、変わったら印の版を進めてメモリの判定を作り直す。
CREATE TRIGGER contents_bump_marks_exclude_patterns AFTER UPDATE OF exclude_patterns ON contents BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (NEW.id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;
