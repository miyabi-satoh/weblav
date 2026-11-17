-- 新しいパスワードの下限 (文字数) を、管理者がサイトの設定で変えられるようにする (→ docs/access.md「パスワードの下限」)。
-- 既定の 8 は `auth::DEFAULT_PASSWORD_MIN_LENGTH` と揃える。1 以上であることは `api::password_policy` で確かめる。
ALTER TABLE site_settings ADD COLUMN password_min_length INTEGER NOT NULL DEFAULT 8;
