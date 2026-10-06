// 無くても動く秘密の値。無いときの扱いは src/mail.ts。
// 欠かせないもの (証明の鍵) は wrangler.jsonc の `secrets.required` に書き、生成した型に入る。
interface __BaseEnv_Env {
	/** Resend の API キー。 */
	RESEND_API_KEY?: string;
	/** 手元で動かすときだけ "1" (`just dev-account-server` が `--var` で渡す)。メールを送らず、リンクをログに出す。 */
	MAIL_LOG_ONLY?: string;
	/** Stripe (→ src/stripe.ts)。秘密の値2つと個人向けの Price 2つがそろったときだけ売る。 */
	STRIPE_SECRET_KEY?: string;
	STRIPE_WEBHOOK_SECRET?: string;
	/** 個人向けの Pro の月額・年額の Price の id。 */
	STRIPE_PERSONAL_MONTHLY_PRICE_ID?: string;
	STRIPE_PERSONAL_YEARLY_PRICE_ID?: string;
	/** 組織向けの年額の Price の id。無いときは組織向けの請求書を受けない。 */
	STRIPE_ORG_PRICE_ID?: string;
	/** Google でのサインイン (→ src/google.ts)。2つそろったときだけ出す。 */
	GOOGLE_CLIENT_ID?: string;
	GOOGLE_CLIENT_SECRET?: string;
	/** Apple でのサインイン (→ src/apple.ts)。4つそろったときだけ出す。秘密鍵は .p8 の中身。 */
	APPLE_TEAM_ID?: string;
	APPLE_KEY_ID?: string;
	APPLE_PRIVATE_KEY?: string;
	APPLE_SERVICE_ID?: string;
}
