-- リンクコンテンツ第一弾。file/folder/group への拡張を見据えて type と parent_id を
-- 今のうちに持たせておくが、値として使うのは今回 'link' のみ。
--
-- parent_id は folder/group 配下にリンクをぶら下げる将来の階層構造のためのカラム。
-- 親(folder/group)が削除されたら配下も道連れで消えてよいので ON DELETE CASCADE にする。
-- 今回は type='link' のみで、parent_id は常に NULL(トップレベル固定)。
--
-- type と visibility に CHECK 制約は置かない。SQLite では CHECK を後から変えられず
-- テーブル再構築が要るのに対し、type ごとの必須カラムの検証(link なら url、
-- folder/archive なら path、file なら blob_hash)は結局アプリケーション層(Rust)が担う
-- ため、列挙値だけ DB 側に二重に持つ意味が薄い。
--
-- position は表示順の手動採番(小さい順)。D&D 並び替えは今回未対応で、
-- 管理画面のフォームで数値を直接編集する運用とする。
CREATE TABLE contents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 'link' | 'file' | 'folder' | 'group' | 'archive'
    type TEXT NOT NULL,
    parent_id INTEGER REFERENCES contents (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    -- link 以外(file/folder/group)は URL を持たないため NULL 可。「link なら必須」は
    -- アプリケーション層(Rust)で検証する。
    url TEXT,
    description TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    -- 'public'(匿名可) | 'authenticated'(要ログイン)。
    -- 子のレベルは親グループ以上(厳しい側)でなければならない。親子を跨ぐ条件のため
    -- CHECK では書けず、アプリケーション層で検証する。
    visibility TEXT NOT NULL DEFAULT 'authenticated',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- トップページのカード表示は position, id 順で列挙するため。
CREATE INDEX idx_contents_position ON contents (position, id);
