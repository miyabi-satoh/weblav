-- フォルダー (ブラウズ型) の中で「表示しない」にしたファイル・サブフォルダー (→ docs/folders.md「表示しないもの」)。
--
-- フォルダーは索引しないので、外したものだけを持つ。サブフォルダーなら中ごと外す。
-- `path` はフォルダーの登録先からの相対パスで、実体の綴りを '/' でつなぐ (canonicalize してから求める)。
-- 登録先を変えたら消す (相対パスが別のものを指すため。`contents::update_content`)。コンテンツを消すと一緒に消える。
CREATE TABLE folder_hidden_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content_id INTEGER NOT NULL REFERENCES contents (id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    is_dir INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (content_id, path)
);
