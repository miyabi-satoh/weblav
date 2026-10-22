-- アーカイブのアイテムのプレビューの情報を、スキャンのときに読んで残す (→ docs/archive.md「スキャン」)。
--
-- 一覧のたびにファイルを開いて画像の大きさを読むと、NAS の数万枚の写真で十数分かかるため。
-- 一覧と検索はここの値を返すだけにし、ファイルを開かない。
-- `file_size` と `modified_ns` は、次のスキャンで変わったファイルだけを読み直すための目印。
-- `file_size` が NULL の行は、まだ読んでいない (このマイグレーションより前に索引した行)。次のスキャンで埋まる。
-- 画像でない・大きさを読めないファイルは `image_width`・`image_height` が NULL。
ALTER TABLE archive_items ADD COLUMN file_size INTEGER;
ALTER TABLE archive_items ADD COLUMN modified_ns INTEGER;
ALTER TABLE archive_items ADD COLUMN image_width INTEGER;
ALTER TABLE archive_items ADD COLUMN image_height INTEGER;
ALTER TABLE archive_items ADD COLUMN is_text INTEGER NOT NULL DEFAULT 0;
