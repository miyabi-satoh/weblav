-- URL の拡張子で開き方が決まらないリンクを、相手の応答 (`Content-Type`・`Content-Disposition`) で
-- ファイルの行・ビューアーで開くため、取ったときに決めたファイル名を覚える (→ docs/ui.md「URL のファイル」)。
-- 中継の口も、この名前の拡張子で型を決める。HTML のページと、対象でない種類は NULL。
ALTER TABLE link_previews ADD COLUMN file_name TEXT;

-- 覚えている行は検証子のまま確かめると 304 になり、名前が埋まらない。ファイルかもしれない行 (HTML でない応答は題が無い) を一度取り直させる。
UPDATE link_previews SET etag = NULL, last_modified = NULL, fresh_until = 0 WHERE title IS NULL;
