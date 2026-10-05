-- 管理者向け機能(コンテンツ管理等)の土台として admin/user のロールを追加する。
-- 既存ユーザーは互換性のため全員 'user' 扱いにし、最初の管理者は
-- `weblav --create-user <name> --admin` で作成する。
ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user'));
