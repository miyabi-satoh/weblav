-- 操作の記録を、共有の中の操作だけでなく、ログインやデータベースを書き換える操作にも広げる (→ docs/operations.md)。
-- 共有の中の操作でない行は共有と場所を持たないので、`root_id`・`root_name`・`paths` の NOT NULL を外すために作り直す。
CREATE TABLE operations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- 'auth' | 'contents' | 'users' | 'shares' | 'settings'。画面の絞り込みに使う (`op` から決まる)。
    category TEXT NOT NULL,
    -- 操作したユーザー。ログインに失敗したときは、打ち込まれた名前のユーザーがいればその id で、いなければ NULL。
    user_id INTEGER,
    -- そのときの名前。ログインに失敗したときは打ち込まれた名前。
    username TEXT NOT NULL,
    op TEXT NOT NULL,
    -- 操作の対象 (コンテンツ・ユーザー・共有など) の id と、そのときの名前。共有の中の操作は下の場所で持つ。
    target_id INTEGER,
    target_name TEXT,
    -- 名前を変えた操作の、変えた後の名前 (`target_name` は変える前の名前)。
    target_renamed_to TEXT,
    -- 対象の中の部分の名前 (アーカイブの軸の名前)。
    target_part TEXT,
    -- 編集で変えた項目の名前の JSON の配列。値は残さない。
    fields TEXT,
    -- ここから共有の中の操作の分。扱った共有と、共有からの相対パス ('/' 区切り) の JSON の配列。
    root_id INTEGER,
    root_name TEXT,
    paths TEXT,
    -- 移動・コピーの置き先のフォルダーと、名前の変更の後の場所。
    dest_root_id INTEGER,
    dest_root_name TEXT,
    dest_path TEXT,
    -- 扱ったファイル・フォルダーの数と、ファイルの大きさの合計。終わるまで分からないものは NULL。
    item_count INTEGER,
    byte_count INTEGER,
    -- 'running' | 'succeeded' | 'failed' | 'cancelled'
    result TEXT NOT NULL,
    -- 失敗したときの API のエラーの code (`error::ErrorCode`)。
    error TEXT
);

INSERT INTO operations
    (id, at, category, user_id, username, op, root_id, root_name, paths, dest_root_id, dest_root_name,
     dest_path, item_count, byte_count, result, error)
SELECT id, at, 'shares', user_id, username, op, root_id, root_name, paths, dest_root_id, dest_root_name,
       dest_path, item_count, byte_count, result, error
FROM share_operations;

DROP TABLE share_operations;

CREATE INDEX idx_operations_at ON operations (at);
