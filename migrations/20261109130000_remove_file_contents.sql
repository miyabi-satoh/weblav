-- アップロードしたファイルをコンテンツとして持つ種類 (`type = 'file'`) を外し、その行と、その種類だけが使っていた列を消す。
-- ファイルは共有の中に置き、ホームなどにはショートカット (`type = 'shortcut'`、`path` で指す) で載せるため。
-- ショートカットのファイル名は `path` から、大きさは先のファイルから取るので、`file_name`・`file_size` も要らない。
-- 実体のファイル (データの置き場の `blobs/`) はこのマイグレーションでは消さない。DB からは触れなくなる。
-- `file` の行には子が無い (親になれるのはグループだけ) ので、消しても `parent_id` で付け替わる行は無い。

DELETE FROM contents WHERE type = 'file';

-- `DROP COLUMN` は列を指す索引があると断られるので、列より先に消す。
DROP INDEX idx_contents_blob_hash;

ALTER TABLE contents DROP COLUMN blob_hash;
ALTER TABLE contents DROP COLUMN file_name;
ALTER TABLE contents DROP COLUMN file_size;
