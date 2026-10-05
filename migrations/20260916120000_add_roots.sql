-- 「公開できるフォルダ」(→ docs/design.md 8.2)。コンテンツに使えるパスを、この配下だけに限る。
--
-- 登録・削除できるのはサーバーの PC の前にいる管理者だけ (ループバック + admin)。
-- パスは canonicalize 済みの絶対パスを入れる (比較が実体パス同士になるようにするため。
-- 検証は `api::roots` で行う)。
-- 入れ子は登録時に拒む。どちらの向きでも、範囲が重なるルートを2つ持つ意味がないため。
CREATE TABLE roots (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
