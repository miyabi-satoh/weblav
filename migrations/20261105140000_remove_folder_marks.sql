-- パスごとの公開・非公開の印をやめ、フォルダーで見せるものを対象と除外のパターンだけで決める (→ docs/folders.md「見せるものの絞り込み」)。
-- 見せるものの決め方が2通りあると、見えない理由を利用者が追えないため。印で隠していたものは見えるようになる。
-- 印を消すので、印の無いときの初期値 (`contents.new_files_public`・`archive_items.initial_public`) も要らない。

-- `DROP COLUMN` は列を指すトリガーがあると断られるので、列と表より先に消す。
DROP TRIGGER contents_bump_marks;
DROP TRIGGER contents_bump_marks_extensions;
DROP TRIGGER contents_bump_marks_list_view;
DROP TRIGGER contents_bump_marks_exclude_patterns;
DROP TRIGGER archive_items_bump_marks_delete;
DROP TRIGGER archive_items_bump_marks_move;
DROP TRIGGER folder_marks_bump_insert;
DROP TRIGGER folder_marks_bump_update;
DROP TRIGGER folder_marks_bump_delete;

DROP TABLE folder_marks;

ALTER TABLE archive_items DROP COLUMN initial_public;
ALTER TABLE contents DROP COLUMN new_files_public;

-- パターンはメモリに持たないが、開いている一覧の結果は作った時点のパターンで絞ってある。
-- 変わったら画面に取り直しを出せるよう (`stale`)、版は残す。
ALTER TABLE archive_versions RENAME COLUMN marks_version TO filter_version;

CREATE TRIGGER contents_bump_filter AFTER UPDATE OF include_patterns, exclude_patterns ON contents BEGIN
    INSERT INTO archive_versions (content_id, filter_version) VALUES (NEW.id, 1)
    ON CONFLICT (content_id) DO UPDATE SET filter_version = filter_version + 1;
END;
