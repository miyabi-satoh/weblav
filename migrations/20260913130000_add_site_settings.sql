-- サイト全体の設定 (→ docs/design.md 10.2)。ホームの見出しに出すサイト名と見出しを持つ。
--
-- 1行だけのテーブルにする (id = 1 固定)。項目ごとに型と既定値を持たせるため、キーと値の表にしない。
-- 未設定は空文字列で表す。画面は空の行を出さない (→ docs/design.md 9.3)。
CREATE TABLE site_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    site_name TEXT NOT NULL DEFAULT '',
    home_heading TEXT NOT NULL DEFAULT ''
);

INSERT INTO site_settings (id) VALUES (1);
