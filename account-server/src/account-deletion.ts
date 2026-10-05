/**
 * アカウントを消す (→ docs/pro.md「アカウントと販売の窓口」)。削除の請求は問い合わせで受け、運営者が `scripts/delete-account.mjs` で流す。
 * 購入の台帳 (purchases) とサブスクの行 (subscriptions) は、外部キーで結び付きだけが外れて残る。サインインの状態・外部のサインインの結び付き・結んだ WebLAV は外部キーで消える。
 * サブスクの解約は、この前にスクリプトが Stripe で行う。
 * どの文も `?1` にメールアドレス (小文字) を取る。スクリプトからも読むので、ほかのモジュールを import しない。
 */
export const DELETE_ACCOUNT_STATEMENTS = [
	'DELETE FROM checkouts WHERE account_id = (SELECT id FROM accounts WHERE email = ?1)',
	// 台帳に残すのは、取引の id・製品・額・日時・取り消したかだけ。
	`UPDATE purchases SET managed_payments = NULL, card_country = NULL, detached_at = unixepoch()
	 WHERE account_id = (SELECT id FROM accounts WHERE email = ?1)`,
	'DELETE FROM email_logins WHERE email = ?1',
	'DELETE FROM accounts WHERE email = ?1'
];
