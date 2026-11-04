-- 対象の拡張子は、フォルダーの表示でも、見せるファイルを絞るようにした (→ docs/folders.md「公開と非公開」)。
-- 印と同じくサーバーのメモリに持つ判定の材料なので、変わったら印の版を進めて作り直す。
CREATE TRIGGER contents_bump_marks_extensions AFTER UPDATE OF extensions ON contents BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (NEW.id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;
