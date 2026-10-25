-- 一覧の表示を使うフォルダーごとに、索引と導出の材料 (`index_version`) と公開・非公開の印 (`marks_version`) の版を持つ。
-- 導出の結果と印をサーバーのメモリに持つので、どちらかが変わったら作り直すため (→ docs/archive.md「導出の結果と印をサーバーのメモリに持つ」)。
-- 版はトリガーで進める。書き換える経路 (スキャン・開いたときの読み直し・軸・辞書・テンプレート・印・設定) が多く、
-- コードの側で進めると1つ漏らしただけで古い一覧や古い公開を返すため。

-- 外部キーは張らない。コンテンツを消したときに連鎖で消える索引の行のトリガーが、消している最中の
-- コンテンツを指す行を足そうとして外部キーの検査に掛かるため。残った行は、コンテンツの番号を使い回さない
-- (`contents.id` は AUTOINCREMENT) ので害が無い。
CREATE TABLE archive_versions (
    content_id INTEGER PRIMARY KEY,
    index_version INTEGER NOT NULL DEFAULT 0,
    marks_version INTEGER NOT NULL DEFAULT 0
);

-- 索引の行
CREATE TRIGGER archive_items_bump_insert AFTER INSERT ON archive_items BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_items_bump_update AFTER UPDATE ON archive_items BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_items_bump_delete AFTER DELETE ON archive_items BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (OLD.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;

-- 軸の定義
CREATE TRIGGER archive_axes_bump_insert AFTER INSERT ON archive_axes BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_axes_bump_update AFTER UPDATE ON archive_axes BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_axes_bump_delete AFTER DELETE ON archive_axes BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (OLD.archive_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;

-- 値の辞書 (軸を通してフォルダーを引く)
CREATE TRIGGER archive_axis_values_bump_insert AFTER INSERT ON archive_axis_values BEGIN
    INSERT INTO archive_versions (content_id, index_version)
    SELECT archive_id, 1 FROM archive_axes WHERE id = NEW.axis_id
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_axis_values_bump_update AFTER UPDATE ON archive_axis_values BEGIN
    INSERT INTO archive_versions (content_id, index_version)
    SELECT archive_id, 1 FROM archive_axes WHERE id = NEW.axis_id
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER archive_axis_values_bump_delete AFTER DELETE ON archive_axis_values BEGIN
    INSERT INTO archive_versions (content_id, index_version)
    SELECT archive_id, 1 FROM archive_axes WHERE id = OLD.axis_id
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;

-- コンテンツの設定のうち、導出の材料 (表示タイトルのテンプレート・登録先) と、印の無いものの公開
CREATE TRIGGER contents_bump_index AFTER UPDATE OF title_template, path ON contents BEGIN
    INSERT INTO archive_versions (content_id, index_version) VALUES (NEW.id, 1)
    ON CONFLICT (content_id) DO UPDATE SET index_version = index_version + 1;
END;
CREATE TRIGGER contents_bump_marks AFTER UPDATE OF new_files_public ON contents BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (NEW.id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;

-- 公開・非公開の印
CREATE TRIGGER folder_marks_bump_insert AFTER INSERT ON folder_marks BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (NEW.content_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;
CREATE TRIGGER folder_marks_bump_update AFTER UPDATE ON folder_marks BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (NEW.content_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;
CREATE TRIGGER folder_marks_bump_delete AFTER DELETE ON folder_marks BEGIN
    INSERT INTO archive_versions (content_id, marks_version) VALUES (OLD.content_id, 1)
    ON CONFLICT (content_id) DO UPDATE SET marks_version = marks_version + 1;
END;
