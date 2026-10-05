-- 索引型アーカイブ(type='archive')を追加する。contents 側の2カラムと、専用の3テーブル。
--
-- 設計の要点: アイテムごとの属性値はどこにも保存しない。archive_items.rel_path から
-- 読み出し時に毎回導出する。中間テーブルとその同期処理が不要になり、軸の定義や
-- 照合語リストを変えた直後から再スキャンを待たずに反映される。

-- 索引対象の拡張子をカンマ区切りで保持する。NULL/空なら全ファイル。
ALTER TABLE contents ADD COLUMN extensions TEXT;

-- 表示タイトルの組み立てテンプレート(例: '{科目}（{資料種別}）')。
-- NULL ならファイル名をそのまま表示する。
ALTER TABLE contents ADD COLUMN title_template TEXT;

-- アーカイブ配下のファイル1件。
CREATE TABLE archive_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    archive_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    -- アーカイブの登録先フォルダからの相対パス。区切りは '/' に正規化して保存する
    -- (Windows と他プラットフォームで同じ値にするため)。
    -- アイテムの同一性はこの値のみで判断する。ファイルを移動・改名すると別のアイテムに
    -- なり、公開フラグは引き継がれない。
    rel_path TEXT NOT NULL,
    -- 0=非公開。スキャンで新しく見つかった行は必ず 0 から始まる。これにより再スキャンは
    -- 常に安全な操作になる(勝手に何かが公開されることはない)。
    -- 索引対象から外れた行はスキャン時に DELETE する。行を消すこと自体が「一度外れたら
    -- 公開フラグを復元しない」の実装になるため、墓標(tombstone)行は持たない。
    published INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (archive_id, rel_path)
);

-- 閲覧時は「このアーカイブの公開アイテムを全件」引く。
CREATE INDEX idx_archive_items_archive_id ON archive_items (archive_id, published);

-- 属性軸の定義。
CREATE TABLE archive_axes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    archive_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    -- 軸の表示名。contents.title_template のプレースホルダーはこの名前で参照するため、
    -- リネーム時は同一トランザクションで title_template も書き換える。
    -- '{' '}' を含めないことはアプリケーション層で検証する。
    name TEXT NOT NULL,
    -- 'dir_level'(フォルダの第N階層) | 'filename_word'(ファイル名に含まれる語)
    source TEXT NOT NULL,
    -- source='dir_level' のときのみ使う。アーカイブ登録先の直下を 1 とする階層番号。
    dir_level INTEGER,
    -- 軸そのものの並び順(絞り込みUIでの左からの順番)。
    position INTEGER NOT NULL DEFAULT 0,
    UNIQUE (archive_id, name)
);

CREATE INDEX idx_archive_axes_archive_id ON archive_axes (archive_id, position, id);

-- 軸の値の辞書。source によって意味が変わる。
--
--   dir_level     : 表示名と並び順の上書きにすぎない。値そのものは rel_path の第N階層の
--                   文字列であり、行が無くても値は成立する。行があれば display_name と
--                   position をそこから取り、無ければ生の値をそのまま表示する。
--                   軸を足した直後から、再スキャンを待たずに値が出る。
--   filename_word : この表の行そのものが照合語リスト。raw_value が照合する語、
--                   position が照合優先度(小さいほど優先)、display_name が表示名。
--                   行が無ければその軸は何にも一致しない。
CREATE TABLE archive_axis_values (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    axis_id INTEGER NOT NULL REFERENCES archive_axes (id) ON DELETE CASCADE,
    raw_value TEXT NOT NULL,
    -- NULL なら raw_value をそのまま表示する。
    display_name TEXT,
    -- 表示順と照合優先度を兼ねる。1本のリストで扱う(並び替えUIを2つに増やさないため)。
    position INTEGER NOT NULL DEFAULT 0,
    UNIQUE (axis_id, raw_value)
);

CREATE INDEX idx_archive_axis_values_axis_id
    ON archive_axis_values (axis_id, position, id);
