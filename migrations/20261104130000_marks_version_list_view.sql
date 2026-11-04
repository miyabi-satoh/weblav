-- ライブラリでないフォルダーは、印の無いものを公開として扱うようにした (→ docs/folders.md「公開と非公開」)。
-- ライブラリとして整理するかが公開の判定の材料に加わったので、変わったら印の版を進めてメモリの判定を作り直す。
CREATE TRIGGER contents_bump_marks_list_view AFTER UPDATE OF list_view ON contents BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (NEW.id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;
