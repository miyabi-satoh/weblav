-- `just spec` (アプリ仕様書生成) 用の開発データシード。
-- 一時DB(WEBLAV_HOMEを一時ディレクトリに向けて起動したweblav)に対して
-- generate-spec.ts が node:sqlite で直接流し込む。Rust側の変更は不要。
--
-- password_hash は "zxcvbnm1" の argon2id ハッシュを固定値としてハードコードしている。
-- ハッシュ文字列自体にsalt/パラメータが埋め込まれているため、hash_password()の
-- 実装が変わらない限り再生成不要。作り直す場合は対話コマンドで一度ユーザーを作り、
-- 開発用DBから password_hash 列を採取する (元の clone で流す。git worktree には data/ が無い):
--   sqlite3 data/dev-runtime/weblav.db \
--     "SELECT password_hash FROM users WHERE username='admin'"

-- admin の recovery_code_hash は scripts/test-account.ts の recoveryCode のハッシュ。
-- 持たせておくのは、管理画面に「まだ作っていない」の帯を出さないため (→ docs/access.md「リカバリコード」)。
INSERT INTO users (username, password_hash, role, recovery_code_hash) VALUES
	('admin', '$argon2id$v=19$m=19456,t=2,p=1$5f4Qisl4f3Qt3EMHJh5wQA$NtazHK/CG5KWnLXHOpJgCD4OwScUoPnNAklMk4UzwdE', 'admin', '$argon2id$v=19$m=19456,t=2,p=1$qAk/18nthJ6XkcDmJhByWQ$aowiQPVbSDVz6u++2lnl+5GDScif+77pXyugElBCzsU'),
	('testuser', '$argon2id$v=19$m=19456,t=2,p=1$gLSPXk1zOWE8M7D88iTUdQ$uBQP1Ld3kTQBRON5/Qo+ybHrIlMsOrZ/+c7VSKtc8R8', 'user', NULL);

-- 環境に依存しない type だけをここでシードする(type='folder'/'archive'/'file' は
-- 実在する絶対パス・実体ファイルが要るため generate-spec.ts 側で追加INSERTする)。
--
-- visibility は匿名閲覧のショット(home_anonymous)のために書き分ける。
-- 'authenticated' はログイン済みにだけ、'public' は未ログインにも見える(→ docs/access.md「公開範囲と匿名閲覧」)。
INSERT INTO contents (type, title, url, description, visibility) VALUES
	('link', 'サンプルリンク1', 'https://example.com/1', 'コンテンツ仕様書用のサンプルデータです。', 'authenticated'),
	('link', 'サンプルリンク2', 'https://example.com/2', NULL, 'authenticated'),
	('link', '公開サンプルリンク', 'https://example.com/public', '未ログインでも見えるコンテンツです。', 'public');

-- 原稿の 05-groups.md のスクリーンショット用: type=link/folder/file/group/archive の
-- カード全種類を1つのgroup配下に揃える。既存のtype=link/folderは再利用せず
-- 専用の子行を新規に作る(home_with_contents/home_anonymous等の既存ショットが
-- ルート直下の並びを前提にしており、既存行を子に移すとそちらが意図せず変化するため)。
-- folder/archive/fileは環境依存のためgenerate-spec.ts側で追加し、parent_idも
-- そちらで設定する。
INSERT INTO contents (type, title, description, visibility) VALUES
	('group', '教材(グループ)', 'グループ閲覧のスクリーンショット用のフィクスチャです。', 'authenticated'),
	('group', '季節講習', 'グループのネスト(カードの種類)確認用の空フィクスチャです。', 'authenticated');
INSERT INTO contents (type, title, url, description, visibility) VALUES
	('link', '英検公式サイト', 'https://www.eiken.or.jp/',
		'グループ内のlinkカードのスクリーンショット用のフィクスチャです。', 'authenticated');

UPDATE contents SET parent_id = (SELECT id FROM contents WHERE title = '教材(グループ)')
	WHERE title IN ('季節講習', '英検公式サイト');

-- 本人のみ・非表示のショット用。本人のみは作成者を入れる (作成者のいない本人のみは生まれない)。
-- home_with_contents (admin) では自分の本人のみに印が付き、admin_contents_list では
-- 作成者の列に testuser が出て、admin_users_delete_confirm (testuser) では件数が出る。
INSERT INTO contents (type, title, url, description, visibility, created_by) VALUES
	('link', '自分用のメモ', 'https://example.com/memo', '作成者にだけ見えるコンテンツです。', 'private',
		(SELECT id FROM users WHERE username = 'admin')),
	('link', 'testuser のメモ', 'https://example.com/testuser-memo', NULL, 'private',
		(SELECT id FROM users WHERE username = 'testuser')),
	('link', '準備中の資料', 'https://example.com/draft', '閲覧の場には誰にも並ばない下書きです。', 'hidden',
		(SELECT id FROM users WHERE username = 'admin'));

-- ほかの行の作成者は admin にする。実際の行は作成者を持つので (→ docs/access.md「ユーザーの削除と作成者」)、
-- admin_contents_list の作成者の列が「削除されたユーザー」で埋まらないようにする。
-- サンプルリンク2 だけは、作成者のユーザーを削除した行の見本として空のまま残す。
UPDATE contents SET created_by = (SELECT id FROM users WHERE username = 'admin')
	WHERE created_by IS NULL AND title <> 'サンプルリンク2';

-- ホームの見出しの例 (→ docs/ui.md「UI 全般」)。業種を思わせない値にする。
UPDATE site_settings SET site_name = 'サンプル事務所', home_heading = '共有の資料' WHERE id = 1;
