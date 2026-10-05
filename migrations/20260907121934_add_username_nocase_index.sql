-- username の照合(ログイン照合・重複チェック)で大文字小文字を無視するためのユニークインデックス。
-- 既存の users.username カラム自体の照合順序(BINARY)は変更しない(SQLiteでは列のCOLLATE変更に
-- テーブル再作成が必要なため)。代わりに COLLATE NOCASE を指定したユニークインデックスを追加し、
-- INSERT時の重複チェックをこのインデックス側に担わせる。ログイン照合側は src/auth.rs のクエリで
-- `WHERE username = ? COLLATE NOCASE` を明示すること(インデックスを使わせるため)。
-- 注意: このマイグレーションは、大文字小文字違いのユーザー名(例: alice / Alice)が既に重複して
-- 存在する環境では失敗する。
--
-- NOCASE は ASCII 範囲のみを大文字小文字無視で比較する(SQLite標準仕様)。username は ASCII 前提。
CREATE UNIQUE INDEX users_username_nocase ON users (username COLLATE NOCASE);
