/**
 * ローカルの e2e と仕様書の撮影で使う管理者。
 * `passwordHash` は `password` の argon2id ハッシュで、scripts/seed.sql の admin と同じ値 (作り直し方も同ファイル)。
 * `recoveryCodeHash` は `recoveryCode` の区切りを除いた形のハッシュ (→ docs/access.md「リカバリコード」)。
 * コードを持たせておくのは、管理画面に「まだ作っていない」の帯を出さないため。
 */
export const TEST_ADMIN = {
	username: 'admin',
	password: 'zxcvbnm1',
	passwordHash:
		'$argon2id$v=19$m=19456,t=2,p=1$5f4Qisl4f3Qt3EMHJh5wQA$NtazHK/CG5KWnLXHOpJgCD4OwScUoPnNAklMk4UzwdE',
	recoveryCode: 'E2E00-RECVR-C0DE0-00001',
	recoveryCodeHash:
		'$argon2id$v=19$m=19456,t=2,p=1$qAk/18nthJ6XkcDmJhByWQ$aowiQPVbSDVz6u++2lnl+5GDScif+77pXyugElBCzsU'
} as const;

/** scripts/seed.sql が入れる一般ユーザー。 */
export const TEST_USER = {
	username: 'testuser',
	password: 'zxcvbnm1'
} as const;
