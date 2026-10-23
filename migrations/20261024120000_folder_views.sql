-- フォルダー (ブラウズ型) とアーカイブ (索引型) を、1つの「フォルダー」にまとめる (→ docs/folders.md)。
-- 種類を作るときに決めさせると、見せ方を変えるには作り直すしかなかったため。
-- 階層を辿る表示はいつでもあり、軸で絞り込む一覧の表示は足し外しできる設定 (`list_view`) になる。
-- 見せるかどうかはパスごとの1つの印 (`folder_marks`) にまとめ、見方は並べ方だけを受け持つ。

-- 一覧の表示 (索引して軸で絞り込む) を使うか。
ALTER TABLE contents ADD COLUMN list_view INTEGER NOT NULL DEFAULT 0;
-- 印の無いファイル (新しく見つかったもの) を公開するか。
ALTER TABLE contents ADD COLUMN new_files_public INTEGER NOT NULL DEFAULT 1;
-- 自動の再スキャンの間隔。'manual' (手動だけ)・'hourly'・'daily'。
ALTER TABLE contents ADD COLUMN auto_rescan TEXT NOT NULL DEFAULT 'manual';
-- 最後にスキャンし終えた時刻 (Unix ミリ秒)。自動の再スキャンの次の回をここから数える。
ALTER TABLE contents ADD COLUMN last_scanned_at INTEGER;

-- 今のアーカイブは、一覧の表示を使い、新しいファイルを公開しない (選んで公開する) フォルダーになる。
UPDATE contents SET type = 'folder', list_view = 1, new_files_public = 0 WHERE type = 'archive';

-- 公開・非公開の印。`path` はフォルダーの登録先からの相対パスで、実体の綴りを '/' でつなぐ。
-- 実効の公開は、パスとその祖先のうち一番深い印で決まり、印が無ければ `contents.new_files_public` に従う。
-- 突き合わせは文字列で行う。印は数万件になりうり (アーカイブで公開したアイテム)、1件ずつ実体のパスを
-- 引き直すと NAS で遅いため。綴りは書くときと照らすときの両方で実体の綴りにそろえる。
-- 登録先を変えたら消す (相対パスが別のものを指すため)。コンテンツを消すと一緒に消える。
CREATE TABLE folder_marks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    is_dir INTEGER NOT NULL,
    public INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (content_id, path)
);

INSERT INTO folder_marks (content_id, path, is_dir, public, created_at)
    SELECT content_id, path, is_dir, 0, created_at FROM folder_hidden_entries;
INSERT INTO folder_marks (content_id, path, is_dir, public, created_at)
    SELECT archive_id, rel_path, 0, 1, unixepoch() FROM archive_items WHERE published = 1;

DROP TABLE folder_hidden_entries;

-- 公開の印は `folder_marks` に移したので、索引は中身の目録だけになる。
DROP INDEX idx_archive_items_archive_id;
ALTER TABLE archive_items DROP COLUMN published;
CREATE INDEX idx_archive_items_archive_id ON archive_items (archive_id);

-- 写真の撮影日 (EXIF、Unix 秒)。日付の軸が使う。無ければ更新日 (`modified_ns`) に落とす。
ALTER TABLE archive_items ADD COLUMN taken_at INTEGER;
-- どの版の読み方で読んだか。読み方を変えたら上げ、古い版で読んだ行を、変わっていなくても読み直す。
-- 1 は撮影日を読む前の版で、今ある行はそれで読んである。
ALTER TABLE archive_items ADD COLUMN read_version INTEGER NOT NULL DEFAULT 1;

-- 索引したフォルダーとその更新日時。一覧の表示を開いたときに、変わったフォルダーだけを読み直す手掛かり。
-- フォルダーは、中にファイルを足したり消したりすると更新日時が変わる。
CREATE TABLE archive_dirs (
    archive_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    rel_path TEXT NOT NULL,
    modified_ns INTEGER,
    PRIMARY KEY (archive_id, rel_path)
);

-- 軸の抽出元 (`archive_axes.source`) に、日付の 'date_year'・'date_month' が加わる。
-- 列に CHECK は無いので、書き換えは要らない。
