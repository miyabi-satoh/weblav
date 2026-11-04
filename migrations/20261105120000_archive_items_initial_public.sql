-- 「新しく見つかったファイルを公開する」は、スキャンで初めて見つけたときの初期値を決める設定にする。
-- 設定を後から変えても、既にあるファイルの公開が変わらないように、見つけた時点の値を行に残す (→ docs/folders.md「公開と非公開」)。
-- 利用者が決めた公開は今までどおり `folder_marks` に持つ。これは、印が無いときの値。
ALTER TABLE archive_items ADD COLUMN initial_public INTEGER NOT NULL DEFAULT 1;

-- 既にある行は、今見えているとおりの値にする。
UPDATE archive_items
SET initial_public = (SELECT new_files_public FROM contents WHERE contents.id = archive_items.archive_id);

-- 初期値も公開の判定の材料で、印と一緒にサーバーのメモリに持つ。行が消える・場所が変わると判定が古くなるので、印の版を進める。
-- 足すときは、その時点の設定と同じ値なので判定は変わらない。
CREATE TRIGGER archive_items_bump_marks_delete AFTER DELETE ON archive_items BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (OLD.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;
CREATE TRIGGER archive_items_bump_marks_move AFTER UPDATE OF rel_path, initial_public ON archive_items BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;
