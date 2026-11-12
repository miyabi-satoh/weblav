-- URL の拡張子で開き方が決まらないリンクを、相手の応答 (`Content-Type`・`Content-Disposition`) で
-- ファイルの行・ビューアーで開くため、取ったときに決めたファイル名を覚える (→ docs/ui.md「URL のファイル」)。
-- 中継の口も、この名前の拡張子で型を決める。HTML のページと、対象でない種類は NULL。
ALTER TABLE link_previews ADD COLUMN file_name TEXT;
