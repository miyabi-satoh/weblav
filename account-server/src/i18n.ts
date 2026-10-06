/**
 * 画面とメールの言語。「クエリの `lang` (`ja`・`en`) > cookie > ブラウザの言語 > 英語」で決める (→ docs/pro.md「アカウントと販売の窓口」)。
 * WebLAV の画面から開くリンクには `lang` が付き、WebLAV で選んでいる言語に合わせる。
 */
import type { Context } from 'hono';
import { getCookie, setCookie } from 'hono/cookie';
import type { Plan } from './link';
import type { SaleRegion } from './pages';
import type { Interval } from './stripe';
import { isHttps } from './util';

export type Lang = 'ja' | 'en';

const COOKIE = 'lang';

/** Managed Payments で売った分の返金の決まり。購入から60日以内は、こちらの決まりより Link のものが優先する。 */
const LINK_REFUND_POLICY =
	'https://support.link.com/questions/requesting-a-refund-for-a-sold-through-link-payment';

function isLang(value: string | undefined): value is Lang {
	return value === 'ja' || value === 'en';
}

/** `Accept-Language` のうち、持っている言語で最も好まれるもの。 */
function preferredLang(header: string | undefined): Lang | undefined {
	const tags = (header ?? '')
		.split(',')
		.map((part) => {
			const [tag, ...params] = part.trim().split(';');
			const q = params.map((p) => p.trim()).find((p) => p.startsWith('q='));
			return { lang: tag.toLowerCase().split('-')[0], q: q ? Number(q.slice(2)) : 1 };
		})
		.filter((t) => isLang(t.lang) && t.q > 0)
		.sort((a, b) => b.q - a.q);
	return tags[0]?.lang as Lang | undefined;
}

/** この要求の言語。`lang` が付いていれば cookie に残し、以後の画面もそれに合わせる。 */
export function resolveLang(c: Context): Lang {
	const asked = c.req.query('lang');
	if (isLang(asked)) {
		setCookie(c, COOKIE, asked, {
			path: '/',
			maxAge: 400 * 24 * 60 * 60,
			sameSite: 'Lax',
			secure: isHttps(c)
		});
		return asked;
	}
	const saved = getCookie(c, COOKIE);
	if (isLang(saved)) return saved;
	return preferredLang(c.req.header('accept-language')) ?? 'en';
}

/** 日付だけを、その言語の形で (日本時間)。 */
export function formatDate(lang: Lang, unix: number): string {
	return new Date(unix * 1000).toLocaleDateString(lang === 'ja' ? 'ja-JP' : 'en-US', {
		timeZone: 'Asia/Tokyo',
		year: 'numeric',
		month: 'short',
		day: 'numeric'
	});
}

const ja = {
	signInTitle: 'サインイン',
	signInHeading: 'WebLAV にサインイン',
	signInLead: 'メールアドレスにサインインのリンクを送ります。',
	signInWithGoogle: 'Google でサインイン',
	signInWithEmail: 'または、メールアドレスにサインインのリンクを送ります。',
	signInConsent:
		'サインインすると、{terms}と{privacy}に同意したことになります (アメリカ合衆国の事業者への個人情報の提供を含みます)。',
	googleFailed: 'Google でサインインできませんでした。もう一度試してください。',
	googleUnconfirmed:
		'この Google アカウントでは、メールアドレスの持ち主を確かめられません。メールアドレスに送るリンクでサインインしてください。',
	googleConflict:
		'このメールアドレスのアカウントには、別の Google アカウントがもう結び付いています。そちらの Google アカウントか、メールアドレスに送るリンクでサインインしてください。',
	email: 'メールアドレス',
	sendLink: 'リンクを送る',
	invalidEmail: 'メールアドレスを確かめてください。',
	tooManyLinks: 'リンクを送った回数が多すぎます。しばらくしてから試してください。',
	mailSentTitle: 'メールを送りました',
	mailSent: (email: string, minutes: number) =>
		`${email} に届いたリンクを開いてください。リンクは${minutes}分で切れます。`,
	mailSentHint: '届かないときは、迷惑メールのフォルダも確かめてください。',
	signIn: 'サインインする',
	linkUnusableTitle: 'このリンクは使えません',
	linkUnusable: 'リンクの期限が切れたか、もう使われています。サインインをやり直してください。',
	signedInAs: (email: string) => `${email} でサインインしています。`,
	signOut: 'サインアウト',
	next: '次へ',
	linkTitle: 'PC を登録',
	linkLead: (name: string) => `「${name}」をこのアカウントに登録します。`,
	relinkLead: (name: string) => `「${name}」を登録し直します。台数は増えません。`,
	linkWarning:
		'自分の WebLAV の画面から開いたのでなければ、押さないでください。他の人の PC に、このアカウントの Pro を使わせることになります。',
	linkCount: (count: number, limit: number) => `登録している PC: ${count} / ${limit} 台`,
	linkButton: 'この PC を登録',
	linkInvalidTitle: 'このリンクは使えません',
	linkInvalid:
		'リンクの形が違うか、作ってから24時間を過ぎました。WebLAV の画面の「この PC を登録」からやり直してください。',
	linkOtherAccount:
		'この PC は別のアカウントに登録されています。そのアカウントでサインインし直してください。',
	linkAtLimitHeading: '登録できる台数の上限に達しています',
	linkAtLimit: (limit: number) =>
		`このアカウントに登録できるのは ${limit} 台までです。使っていない PC の登録を解除してから、このページを開き直してください。`,
	linkedTitle: '登録しました',
	linkedLead:
		'WebLAV の画面に戻ってください。インターネットにつながる PC なら、数秒で Pro に切り替わります。',
	linkedCodeLead:
		'インターネットにつながらない PC では、次のコードを WebLAV の画面に入れてください。同じコードをメールでも送りました。',
	linkedCodeMailSubject: 'WebLAV の登録コード',
	linkedCodeMailBody: (name: string, code: string) =>
		[
			`「${name}」をアカウントに登録しました。`,
			'',
			'インターネットにつながらない PC では、次のコードを WebLAV の画面に入れてください。',
			'',
			code,
			'',
			'心当たりが無ければ、アカウントのページでこの PC の登録を解除してください。'
		].join('\n'),
	releaseTitle: 'PC の登録を解除',
	releaseLead: 'WebLAV の画面で解除した登録を、アカウントに伝えます。サインインは要りません。',
	releaseCodeLabel: 'WebLAV の画面に出たコード',
	releaseButton: '登録を解除',
	releasedTitle: '登録を解除しました',
	released: '登録できる台数が1台空きました。',
	releaseInvalid:
		'コードの形が違います。WebLAV の画面に出たコードを、もう一度読み取るか打ち込んでください。',
	noProHeading: 'このアカウントには Pro がありません',
	noPro: 'ほかのメールアドレスで申し込んだときは、そのアドレスでサインインし直してください。',
	noProBuy: 'Pro を申し込むと、そのままこの PC を登録できます。',
	seePricing: 'Pro の料金と申し込み',
	buyTitle: 'Pro を申し込む',
	// 特定商取引法 12条の6 の最終確認画面 (→ docs/pro.md「売り方」)。価格は本番の Stripe の Price と、紹介・規約類 (site/) に合わせる。
	confirmTitle: 'お申し込み内容の最終確認',
	confirmPlanLabel: 'プラン',
	confirmPlan: (interval: Interval): string =>
		interval === 'year' ? 'WebLAV Pro (個人向け)・年額' : 'WebLAV Pro (個人向け)・月額',
	confirmPriceLabel: '価格',
	confirmPrice: (interval: Interval): string =>
		interval === 'year' ? '4,800 円 / 年 (税込み)' : '480 円 / 月 (税込み)',
	confirmRenewLabel: '更新',
	confirmRenew: (interval: Interval): string =>
		interval === 'year'
			? '1年ごとに自動で更新し、次からも1年ごとに同じ日に 4,800 円を払います。'
			: '1か月ごとに自動で更新し、次からも毎月同じ日に 480 円を払います。',
	confirmPcsLabel: '登録できる PC',
	confirmPcs: '3 台まで',
	confirmPaymentLabel: '支払い',
	confirmPayment: {
		domestic: '次の画面 (Stripe) でカードで払います。払うとすぐ、このアカウントに Pro が付きます。',
		overseas:
			'次の画面で払い方を選びます。販売と決済は Link (Sold through Link, LLC) が代わりに行い、カードの明細には「LINK.COM*」と出ます。お住まいの国の通貨に換えた額で表示されることがあります。払うとすぐ、このアカウントに Pro が付きます。'
	} as Record<SaleRegion, string>,
	confirmCancelLabel: '解約と返金',
	confirmCancel: {
		domestic:
			'アカウントのページからいつでも解約でき、払い終えた期間の終わりまで Pro のまま使えます。払い終えた期間は、ご都合による返金はできません。二重に請求したとき、決済の処理を誤ったとき、支払いが済んだのに Pro が付かなかったときは返金します。',
		overseas: `アカウントのページからいつでも解約でき、払い終えた期間の終わりまで Pro のまま使えます。払い終えた期間は、ご都合による返金はできません。二重に請求したとき、決済の処理を誤ったとき、支払いが済んだのに Pro が付かなかったときは返金します。ただし購入から60 日以内は、[Link の返金ポリシー](${LINK_REFUND_POLICY})によって返金されることがあります。`
	} as Record<SaleRegion, string>,
	buyConsent: '{terms}・{privacy}・{tokushoho}に同意のうえ、進んでください。',
	confirmButton: '申し込みを確定して支払いへ',
	changePlan: 'プランを変える',
	checkoutNote: (tokushoho: string) =>
		`期間ごとに自動で更新し、同じ額を払います。支払いが済むとすぐ、WebLAV のアカウントに Pro が付きます。解約はアカウントのページからいつでもでき、払い終えた期間の終わりまで使えます。払い終えた期間は、ご都合による返金はできません。二重に請求したとき、決済の処理を誤ったとき、支払いが済んだのに Pro が付かなかったときは、その分を返金します。詳しくは[特定商取引法に基づく表記](${tokushoho})をご覧ください。`,
	notForSale: 'いまは Pro を買えません。',
	buyBusy: '支払いの画面を用意しています。少ししてから、もう一度押してください。',
	checkingPurchase: '支払いを確かめています。このままお待ちください。',
	purchaseNotYet:
		'支払いをまだ確かめられていません。少ししてから確かめ直してください。買い直す前に、領収のメールが届いていないかも確かめてください。',
	checkAgain: 'もう一度確かめる',
	accountTitle: 'アカウント',
	planName: { personal: '個人向け', organization: '組織向け' } as Record<Plan, string>,
	plans: (plans: Plan[]): string => `Pro (${plans.map((p) => ja.planName[p]).join('・')})`,
	noPlans: 'Pro はありません。',
	planUntil: (plan: Plan, date: string): string =>
		`Pro (${ja.planName[plan]}): ${date} まで払い済み`,
	manageBilling: '支払いを管理する (解約・支払い方法・領収書)',
	installationsHeading: '登録している PC',
	noInstallations:
		'登録している PC はまだありません。WebLAV のサイト設定の「Pro」から登録できます。',
	installationLine: (name: string, checked: string | undefined) =>
		checked ? `${name} (最後に確かめた日: ${checked})` : `${name} (ネットで確かめていません)`,
	removeButton: '登録を解除',
	overLimitShort: '上限を超えた分',
	removedPending: (until: string) =>
		`登録を解除しました。その PC が受け取るまで、長くて ${until} まで台数に数えます。`,
	overLimit: (limit: number) =>
		`登録している PC が上限 (${limit} 台) を超えています。超えた分 (新しく登録したもの) は Pro になりません。使っていない PC の登録を解除してください。`,
	tooManyTitle: 'しばらくお待ちください',
	tooManyLink: '登録の操作の回数が多すぎます。1分ほどしてから開き直してください。',
	tooManyRelease: '登録の解除の回数が多すぎます。1分ほどしてから押し直してください。',
	about: 'WebLAV について',
	terms: '利用規約',
	privacy: 'プライバシーポリシー',
	tokushoho: '特定商取引法に基づく表記',
	transferTitle: 'Pro を別のアカウントへ移す',
	transferLead:
		'担当者が替わるときなどに、このアカウントの Pro をすべて別のメールアドレスのアカウントへ移します。',
	transferTo: '移し先のメールアドレス',
	transferToSelf: '今サインインしているアドレスとは別のアドレスを入れてください。',
	transferConfirm: (plans: string, to: string) => `${plans} を ${to} へ移します。`,
	transferWarning:
		'移すと、このアカウントには Pro が残りません。登録している PC も移し先へ移ります。戻すには、移し先のアカウントから移し直してもらいます。支払いの宛先と方法は変わらないので、移し先の人がアカウントのページの「支払いを管理する」から直してください。',
	transferButton: '移す',
	transferBack: 'アドレスを入れ直す',
	transferredTitle: 'Pro を移しました',
	transferred: (to: string) => `${to} へ移し、両方のアドレスにメールで知らせました。`,
	transferredNotNotified: (to: string) =>
		`${to} へ移しました。知らせのメールは送れませんでした。移し先の人に、このアドレスでサインインするよう伝えてください。`,
	tooManyTransfers: '移した回数が多すぎます。しばらくしてから試してください。',
	backToAccount: 'アカウントへ戻る',
	transferredFromSubject: 'WebLAV の Pro を移しました',
	transferredFromBody: (to: string) =>
		[
			`あなたの WebLAV のアカウントの Pro を ${to} へ移しました。`,
			'',
			'登録していた PC も移し先へ移りました。'
		].join('\n'),
	transferredToSubject: 'WebLAV の Pro が移されました',
	transferredToBody: (from: string, link: string) =>
		[
			`${from} から、このメールアドレスの WebLAV のアカウントへ Pro が移されました。`,
			'',
			'次のページで、このアドレスでサインインすると確かめられます。PC を登録するときも、このアドレスでサインインしてください。',
			'',
			link
		].join('\n'),
	mailSubject: 'WebLAV にサインイン',
	mailBody: (link: string, minutes: number) =>
		[
			'WebLAV のアカウントにサインインするには、次のリンクを開いてください。',
			'',
			link,
			'',
			`リンクは${minutes}分で切れます。心当たりが無ければ、このメールは無視してください。`
		].join('\n')
};

const en: typeof ja = {
	signInTitle: 'Sign in',
	signInHeading: 'Sign in to WebLAV',
	signInLead: 'We will email you a sign-in link.',
	signInWithGoogle: 'Sign in with Google',
	signInWithEmail: 'Or we can email you a sign-in link.',
	signInConsent:
		'By signing in, you agree to the {terms} and the {privacy}, including providing your personal information to businesses in the United States.',
	googleFailed: "We couldn't sign you in with Google. Please try again.",
	googleUnconfirmed:
		"We can't confirm who owns the email address of this Google account. Please sign in with a link sent to your email address.",
	googleConflict:
		'Another Google account is already linked to the account for this email address. Sign in with that Google account or with a link sent to your email address.',
	email: 'Email address',
	sendLink: 'Send link',
	invalidEmail: 'Check your email address.',
	tooManyLinks: 'Too many links have been sent. Please try again later.',
	mailSentTitle: 'Check your email',
	mailSent: (email: string, minutes: number) =>
		`Open the link we sent to ${email}. The link expires in ${minutes} minutes.`,
	mailSentHint: "If it doesn't arrive, check your spam folder.",
	signIn: 'Sign in',
	linkUnusableTitle: 'This link cannot be used',
	linkUnusable: 'The link has expired or has already been used. Please sign in again.',
	signedInAs: (email: string) => `Signed in as ${email}.`,
	signOut: 'Sign out',
	next: 'Next',
	linkTitle: 'Link a PC',
	linkLead: (name: string) => `Link "${name}" to this account.`,
	relinkLead: (name: string) => `Link "${name}" again. It doesn't count as another PC.`,
	linkWarning:
		"Don't continue unless you opened this page from your own WebLAV. Otherwise someone else's PC would use the Pro on this account.",
	linkCount: (count: number, limit: number) => `Linked PCs: ${count} of ${limit}`,
	linkButton: 'Link this PC',
	linkInvalidTitle: 'This link cannot be used',
	linkInvalid:
		'The link is malformed or more than 24 hours old. Start again from "Link this PC" in WebLAV.',
	linkOtherAccount: 'This PC is linked to another account. Sign in again with that account.',
	linkAtLimitHeading: 'You have reached the limit',
	linkAtLimit: (limit: number) =>
		`This account can link up to ${limit} PCs. Unlink one you no longer use, then open this page again.`,
	linkedTitle: 'Linked',
	linkedLead:
		'Go back to WebLAV. On a PC with internet access, it switches to Pro in a few seconds.',
	linkedCodeLead:
		'On a PC without internet access, enter the following code in WebLAV. We also emailed it to you.',
	linkedCodeMailSubject: 'Your WebLAV code',
	linkedCodeMailBody: (name: string, code: string) =>
		[
			`"${name}" has been linked to your account.`,
			'',
			'On a PC without internet access, enter the following code in WebLAV.',
			'',
			code,
			'',
			"If you don't recognize this, unlink the PC on your account page."
		].join('\n'),
	releaseTitle: 'Unlink a PC',
	releaseLead: 'Tells your account about a PC you unlinked in WebLAV. No sign-in is needed.',
	releaseCodeLabel: 'Code shown in WebLAV',
	releaseButton: 'Unlink',
	releasedTitle: 'Unlinked',
	released: 'You can now link one more PC.',
	releaseInvalid: 'The code is malformed. Scan or enter the code shown in WebLAV again.',
	noProHeading: "This account doesn't have Pro",
	noPro: 'If you subscribed with another email address, sign in again with that address.',
	noProBuy: 'Subscribe to Pro and link this PC right away.',
	seePricing: 'Pro pricing and subscription',
	buyTitle: 'Subscribe to Pro',
	confirmTitle: 'Review your order',
	confirmPlanLabel: 'Plan',
	confirmPlan: (interval: Interval) =>
		interval === 'year' ? 'WebLAV Pro (personal), yearly' : 'WebLAV Pro (personal), monthly',
	confirmPriceLabel: 'Price',
	confirmPrice: (interval: Interval) =>
		interval === 'year' ? '4,800 yen / year (tax included)' : '480 yen / month (tax included)',
	confirmRenewLabel: 'Renewal',
	confirmRenew: (interval: Interval) =>
		interval === 'year'
			? 'Renews automatically every year. You pay 4,800 yen on the same date each year.'
			: 'Renews automatically every month. You pay 480 yen on the same date each month.',
	confirmPcsLabel: 'PCs you can link',
	confirmPcs: 'Up to 3',
	confirmPaymentLabel: 'Payment',
	confirmPayment: {
		domestic:
			'You pay by card on the next page (Stripe). Pro is added to this account as soon as you pay.',
		overseas:
			'You choose how to pay on the next page. The sale and payment are handled on our behalf by Link (Sold through Link, LLC), and your card statement shows "LINK.COM*". The amount may be shown in your local currency. Pro is added to this account as soon as you pay.'
	},
	confirmCancelLabel: 'Cancellation and refunds',
	confirmCancel: {
		domestic:
			'You can cancel at any time on your account page and keep using Pro until the end of the paid period. Paid periods are not refunded for personal reasons. We refund the amount if we charged you twice, made an error in processing the payment, or Pro was not added after your payment went through.',
		overseas: `You can cancel at any time on your account page and keep using Pro until the end of the paid period. Paid periods are not refunded for personal reasons. We refund the amount if we charged you twice, made an error in processing the payment, or Pro was not added after your payment went through. Within 60 days of purchase, however, you may get a refund under [Link's refund policy](${LINK_REFUND_POLICY}).`
	},
	buyConsent: 'By continuing, you agree to the {terms}, the {privacy}, and the {tokushoho}.',
	confirmButton: 'Confirm and continue to payment',
	changePlan: 'Change plan',
	checkoutNote: (tokushoho: string) =>
		`It renews automatically each period at the same price. Pro is added to your WebLAV account as soon as the payment is complete. You can cancel at any time on your account page and keep using Pro until the end of the paid period. Paid periods are not refunded for personal reasons. We refund the amount if we charged you twice, made an error in processing the payment, or Pro was not added after your payment went through. For details, see the [Specified Commercial Transactions Act notice](${tokushoho}).`,
	notForSale: 'Pro is not available for purchase right now.',
	buyBusy: 'Preparing the payment page. Please try again in a moment.',
	checkingPurchase: 'Confirming your payment. Please wait.',
	purchaseNotYet:
		"We couldn't confirm your payment yet. Please check again in a moment. Before buying again, check whether a receipt email has arrived.",
	checkAgain: 'Check again',
	accountTitle: 'Account',
	planName: { personal: 'personal', organization: 'organization' },
	plans: (plans: Plan[]) => `Pro (${plans.map((p) => en.planName[p]).join(', ')})`,
	noPlans: 'No Pro.',
	planUntil: (plan: Plan, date: string) => `Pro (${en.planName[plan]}): paid through ${date}`,
	manageBilling: 'Manage billing (cancel, payment method, receipts)',
	installationsHeading: 'Linked PCs',
	noInstallations: 'No PC is linked yet. You can link one from "Pro" in the WebLAV site settings.',
	installationLine: (name: string, checked: string | undefined) =>
		checked ? `${name} (last checked: ${checked})` : `${name} (not checked online)`,
	removeButton: 'Unlink',
	overLimitShort: 'over the limit',
	removedPending: (until: string) =>
		`Unlinked. It counts toward the limit until the PC receives this, at most until ${until}.`,
	overLimit: (limit: number) =>
		`You have more linked PCs than the limit (${limit}). The ones over the limit (linked most recently) don't get Pro. Unlink PCs you no longer use.`,
	tooManyTitle: 'Please wait',
	tooManyLink: 'Too many link attempts. Please reopen this page in a minute.',
	tooManyRelease: 'Too many unlink attempts. Please try again in a minute.',
	about: 'About WebLAV',
	terms: 'Terms of Use',
	privacy: 'Privacy Policy',
	tokushoho: 'Specified Commercial Transactions Act notice',
	transferTitle: 'Move Pro to another account',
	transferLead:
		'Moves all Pro on this account to the account for another email address, for example when the person in charge changes.',
	transferTo: 'Email address to move to',
	transferToSelf: 'Enter an address other than the one you are signed in with.',
	transferConfirm: (plans: string, to: string) => `Move ${plans} to ${to}.`,
	transferWarning:
		'After moving, this account has no Pro. Linked PCs move too. To get it back, the other account has to move it back. The billing contact and payment method stay the same, so the new owner should update them with "Manage billing" on the account page.',
	transferButton: 'Move',
	transferBack: 'Enter the address again',
	transferredTitle: 'Pro has been moved',
	transferred: (to: string) => `Moved to ${to}. We emailed both addresses.`,
	transferredNotNotified: (to: string) =>
		`Moved to ${to}, but we couldn't send the notice emails. Ask the new owner to sign in with this address.`,
	tooManyTransfers: 'Too many moves. Please try again later.',
	backToAccount: 'Back to account',
	transferredFromSubject: 'Your WebLAV Pro has been moved',
	transferredFromBody: (to: string) =>
		[
			`The Pro on your WebLAV account has been moved to ${to}.`,
			'',
			'The linked PCs have moved too.'
		].join('\n'),
	transferredToSubject: 'WebLAV Pro has been moved to you',
	transferredToBody: (from: string, link: string) =>
		[
			`${from} has moved WebLAV Pro to the account for this email address.`,
			'',
			'Sign in with this address on the following page to see it. Use this address to sign in when you link a PC, too.',
			'',
			link
		].join('\n'),
	mailSubject: 'Sign in to WebLAV',
	mailBody: (link: string, minutes: number) =>
		[
			'Open the following link to sign in to your WebLAV account.',
			'',
			link,
			'',
			`The link expires in ${minutes} minutes. If you didn't request this, you can ignore this email.`
		].join('\n')
};

export const messages: Record<Lang, typeof ja> = { ja, en };
