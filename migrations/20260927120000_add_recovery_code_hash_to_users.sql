-- 管理者のリカバリコードのハッシュ (→ docs/design.md 7.7)。未作成と `user` は NULL。
ALTER TABLE users ADD COLUMN recovery_code_hash TEXT;
