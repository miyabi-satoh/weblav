-- 共有の中のファイルを書き換えた操作の記録 (→ `api::shares::log`)。管理者が、誰がいつ何を動かし・消したかを
-- 後から確かめられるようにするため。見るだけの操作 (一覧・ダウンロード) は残さない。
--
-- 外部キーは張らない。ユーザーや共有を消した後も記録を残すため、名前はそのときの値を写して持つ。
-- `op` は 'upload'・'mkdir'・'rename'・'move'・'copy'・'delete'・'list_on_home'、
-- `result` は 'running'・'succeeded'・'failed'・'cancelled'。検証は Rust の型で行う (CHECK を付けると、
-- 値を足すたびにテーブルを作り直すことになるため)。
-- 'running' は、コピー・移動・削除のように裏で続く処理の途中。起動し直したときに残っていれば、
-- 途中で止まったので 'failed' にする (処理はメモリの中にしか無く、続きをしないため)。
-- 365 日より古い行は、起動したときと1日ごとに消す。
CREATE TABLE share_operations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    user_id INTEGER,
    username TEXT NOT NULL,
    op TEXT NOT NULL,
    root_id INTEGER NOT NULL,
    root_name TEXT NOT NULL,
    -- 扱った場所の、共有からの相対パス ('/' 区切り) の JSON の配列。いくつかを選んで動かした・消したときは、
    -- 選んだものを並べる (中のファイルまでは並べない)。
    paths TEXT NOT NULL,
    -- 移動・コピーの置き先のフォルダーと、名前の変更の後の場所。
    dest_root_id INTEGER,
    dest_root_name TEXT,
    dest_path TEXT,
    -- 扱ったファイル・フォルダーの数と、ファイルの大きさの合計。終わるまで分からないものは NULL。
    item_count INTEGER,
    byte_count INTEGER,
    result TEXT NOT NULL,
    -- 失敗したときの API のエラーの code (`error::ErrorCode`)。
    error TEXT
);

CREATE INDEX idx_share_operations_at ON share_operations (at);
