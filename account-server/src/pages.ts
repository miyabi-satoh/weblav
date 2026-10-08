import { html } from 'hono/html';
import type { HtmlEscapedString } from 'hono/utils/html';
import { formatDate, messages, type Lang } from './i18n';
import type { Plan } from './link';
import type { Interval } from './stripe';
import { ACCOUNT, ACCOUNT_HOME, PLAN_PATH, PRICING_PATH, TRANSFER_PATH } from './util';

/** 規約類 (site/) のパス。 */
export const LEGAL_PAGES = {
	terms: '/terms/',
	privacy: '/privacy/',
	tokushoho: '/tokushoho/'
} as const;

type Body = HtmlEscapedString | Promise<HtmlEscapedString>;

// 見た目は WebLAV の決めごと (docs/ui.md「UI 全般」) の差し色と中性色に合わせる。
// 足もとの紹介と規約類 (site/) は日本語だけなので、英語の画面からも同じ所へリンクする。
function page(lang: Lang, title: string, body: Body) {
	const t = messages[lang];
	return html`<!doctype html>
		<html lang="${lang}">
			<head>
				<meta charset="utf-8" />
				<meta name="viewport" content="width=device-width, initial-scale=1" />
				<meta name="referrer" content="no-referrer" />
				<title>${title} - WebLAV</title>
				<style>
					:root {
						color-scheme: light dark;
						--accent: #146e6b;
						--fg: #1f2328;
						--muted: #59636e;
						--bg: #ffffff;
						--border: #d1d9e0;
					}
					@media (prefers-color-scheme: dark) {
						:root {
							--accent: #3fb9b3;
							--fg: #e6edf3;
							--muted: #9198a1;
							--bg: #0d1117;
							--border: #3d444d;
						}
					}
					body {
						margin: 0;
						background: var(--bg);
						color: var(--fg);
						font-family: system-ui, sans-serif;
						line-height: 1.7;
						/* 日本語を文節の切れ目で折り返す (対応していないブラウザでは、ふつうの折り返し)。 */
						word-break: auto-phrase;
					}
					main {
						max-width: 28rem;
						margin: 3rem auto;
						padding: 0 1rem;
					}
					h1 {
						font-size: 1.25rem;
					}
					h2 {
						font-size: 1rem;
						margin-top: 2rem;
					}
					.muted {
						color: var(--muted);
						font-size: 0.875rem;
					}
					/* 返しのコード (15文字)。スマートフォンの幅でも1行に収め、途中で折り返さない。 */
					.code {
						font-size: 1.5rem;
						font-weight: bold;
						letter-spacing: 0.05em;
						font-family: ui-monospace, monospace;
						white-space: nowrap;
					}
					label {
						display: block;
						margin-bottom: 0.25rem;
					}
					input {
						box-sizing: border-box;
						width: 100%;
						min-height: 44px;
						padding: 0 0.75rem;
						font: inherit;
						border: 1px solid var(--border);
						border-radius: 6px;
						background: transparent;
						color: inherit;
						word-break: normal;
					}
					button {
						min-height: 44px;
						margin-top: 1rem;
						padding: 0 1.25rem;
						font: inherit;
						border: 0;
						border-radius: 6px;
						background: var(--accent);
						color: #fff;
						cursor: pointer;
					}
					a {
						color: var(--accent);
					}
					/* Google のブランドの決まりに合わせ、白地に枠と G のロゴを置く。 */
					a.google {
						display: inline-flex;
						align-items: center;
						gap: 0.75rem;
						min-height: 44px;
						padding: 0 1.25rem;
						border: 1px solid var(--border);
						border-radius: 6px;
						background: #fff;
						color: #1f1f1f;
						text-decoration: none;
					}
					/* Apple の決まり (HIG の Sign in with Apple) に合わせ、明るい地では黒、暗い地では白にする。 */
					a.apple {
						display: inline-flex;
						align-items: center;
						gap: 0.75rem;
						min-height: 44px;
						padding: 0 1.25rem;
						border-radius: 6px;
						background: #000;
						color: #fff;
						text-decoration: none;
					}
					@media (prefers-color-scheme: dark) {
						a.apple {
							background: #fff;
							color: #000;
						}
					}
					.providers {
						display: flex;
						flex-wrap: wrap;
						gap: 0.75rem;
					}
					.account {
						margin-top: 2.5rem;
					}
					/* 文の途中で折り返してボタンと並ばないよう、ボタンは次の行に置く。 */
					.account button {
						display: block;
					}
					footer {
						margin-top: 3rem;
						display: flex;
						flex-wrap: wrap;
						gap: 0.25rem 1rem;
					}
					footer a {
						color: var(--muted);
					}
					/* 申し込みの条件の枠。項目名と中身を縦に重ね、スマートフォンの幅でも読める形にする。 */
					dl.order {
						margin: 1.5rem 0;
						padding: 1rem 1.25rem;
						border: 1px solid var(--border);
						border-radius: 8px;
					}
					dl.order dt {
						font-weight: bold;
					}
					dl.order dd {
						margin: 0 0 0.75rem;
					}
					dl.order dd:last-child {
						margin-bottom: 0;
					}
					/* 料金ページへ進むリンク。押す先が別の画面なので、見た目はボタンに揃える。 */
					a.action {
						display: inline-flex;
						align-items: center;
						min-height: 44px;
						padding: 0 1.25rem;
						border-radius: 6px;
						background: var(--accent);
						color: #fff;
						text-decoration: none;
					}
					button.secondary {
						background: transparent;
						color: var(--accent);
						border: 1px solid var(--border);
					}
				</style>
			</head>
			<body>
				<main>
					${body}
					<footer class="muted">
						<a href="/">${t.about}</a>
						<a href="${LEGAL_PAGES.terms}">${t.terms}</a>
						<a href="${LEGAL_PAGES.privacy}">${t.privacy}</a>
						<a href="${LEGAL_PAGES.tokushoho}">${t.tokushoho}</a>
					</footer>
				</main>
			</body>
		</html>`;
}

/** 文言の `{terms}` などを、その規約類へのリンクにする。 */
function withLegalLinks(lang: Lang, text: string) {
	const t = messages[lang];
	return text
		.split(/\{(terms|privacy|tokushoho)\}/)
		.map((part, i) =>
			i % 2 === 1
				? html`<a href="${LEGAL_PAGES[part as keyof typeof LEGAL_PAGES]}"
						>${t[part as keyof typeof LEGAL_PAGES]}</a
					>`
				: part
		);
}

/** 文言の `[文](https://…)` をリンクにする。文言はこちらで書くもので、利用者の入力は通さない。 */
function withLinks(text: string) {
	const parts = text.split(/\[([^\]]+)\]\((https:\/\/[^)\s]+)\)/);
	return parts.map((part, i) =>
		i % 3 === 1 ? html`<a href="${parts[i + 1]}">${part}</a>` : i % 3 === 2 ? '' : part
	);
}

// Google の標準の G のロゴ (Sign in with Google のブランドの決まり)。
const GOOGLE_LOGO = html`<svg width="18" height="18" viewBox="0 0 48 48" aria-hidden="true">
	<path
		fill="#EA4335"
		d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"
	/>
	<path
		fill="#4285F4"
		d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"
	/>
	<path
		fill="#FBBC05"
		d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z"
	/>
	<path
		fill="#34A853"
		d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"
	/>
</svg>`;

// Apple のロゴ。形は Apple Design Resources の Sign in with Apple のボタンと同じ。色は文言と同じ (HIG により黒か白だけ)。
const APPLE_LOGO = html`<svg width="18" height="18" viewBox="4.48 9 22 22" aria-hidden="true">
	<path
		fill="currentColor"
		d="M15.71 14.885c.858 0 1.933-.58 2.573-1.353.58-.7 1.002-1.679 1.002-2.657 0-.133-.012-.266-.036-.375-.954.036-2.102.64-2.79 1.45-.544.616-1.039 1.582-1.039 2.572 0 .145.024.29.036.339.06.012.157.024.254.024ZM12.69 29.5c1.172 0 1.691-.785 3.153-.785 1.486 0 1.812.76 3.116.76 1.28 0 2.138-1.183 2.947-2.342.906-1.329 1.28-2.634 1.305-2.694-.085-.024-2.537-1.027-2.537-3.841 0-2.44 1.933-3.539 2.042-3.624-1.28-1.836-3.225-1.884-3.757-1.884-1.437 0-2.609.87-3.346.87-.797 0-1.848-.822-3.092-.822-2.367 0-4.771 1.957-4.771 5.653 0 2.295.894 4.723 1.993 6.293.942 1.329 1.764 2.416 2.947 2.416Z"
	/>
</svg>`;

/** `google`・`apple`: その方法でサインインできるとき (→ src/google.ts・src/apple.ts) に、そのボタンを先に出す。 */
export function signInPage(
	lang: Lang,
	next: string,
	{
		error,
		google = false,
		apple = false
	}: { error?: string; google?: boolean; apple?: boolean } = {}
) {
	const t = messages[lang];
	const query = new URLSearchParams({ next });
	return page(
		lang,
		t.signInTitle,
		html`<h1>${t.signInHeading}</h1>
			${error ? html`<p role="alert">${error}</p>` : ''}
			${
				google || apple
					? html`<p class="providers">
								${
									google
										? html`<a class="google" href="${ACCOUNT}/login/google?${query}"
												>${GOOGLE_LOGO}${t.signInWithGoogle}</a
											>`
										: ''
								}
								${
									apple
										? html`<a class="apple" href="${ACCOUNT}/login/apple?${query}"
												>${APPLE_LOGO}${t.signInWithApple}</a
											>`
										: ''
								}
							</p>
							<p>${t.signInWithEmail}</p>`
					: html`<p>${t.signInLead}</p>`
			}
			<form method="post" action="${ACCOUNT}/login/email">
				<label for="email">${t.email}</label>
				<input id="email" name="email" type="email" autocomplete="email" required />
				<input type="hidden" name="next" value="${next}" />
				<button>${t.sendLink}</button>
			</form>
			<p class="muted">${withLegalLinks(lang, t.signInConsent)}</p>`
	);
}

export function mailSentPage(lang: Lang, email: string, minutes: number) {
	const t = messages[lang];
	return page(
		lang,
		t.mailSentTitle,
		html`<h1>${t.mailSentTitle}</h1>
			<p>${t.mailSent(email, minutes)}</p>
			<p class="muted">${t.mailSentHint}</p>`
	);
}

/** メールのリンクを開いたところ。開いただけではサインインしない (→ POST /login/email/verify)。 */
export function confirmSignInPage(lang: Lang, token: string) {
	const t = messages[lang];
	return page(
		lang,
		t.signInTitle,
		html`<h1>${t.signInHeading}</h1>
			<form method="post" action="${ACCOUNT}/login/email/verify">
				<input type="hidden" name="token" value="${token}" />
				<button>${t.signIn}</button>
			</form>`
	);
}

/** `backToAccount` は、アカウントのページから来る画面に付ける。行き止まりにしないため。 */
export function messagePage(lang: Lang, title: string, message: string, backToAccount = false) {
	return page(
		lang,
		title,
		html`<h1>${title}</h1>
			<p>${message}</p>
			${backToAccount ? html`<p><a href="${ACCOUNT_HOME}">${messages[lang].backToAccount}</a></p>` : ''}`
	);
}

/** 申し込みの最終確認と同じ事項の並び (特定商取引法 12条の6)。2つの画面で同じ見た目に保つ。 */
function orderList(rows: [string, string][]) {
	return html`<dl class="order">
		${rows.map(
			([label, value]) =>
				html`<dt>${label}</dt>
					<dd>${withLinks(value)}</dd>`
		)}
	</dl>`;
}

function signedInAs(lang: Lang, email: string, next: string) {
	const t = messages[lang];
	return html`<form method="post" action="${ACCOUNT}/logout" class="muted account">
		${t.signedInAs(email)}
		<input type="hidden" name="next" value="${next}" />
		<button class="secondary">${t.signOut}</button>
	</form>`;
}

/** 国内 (Stripe で直接売る) と海外 (Managed Payments で、Link が代わりに売る)。売り方で説明が変わる。 */
export type SaleRegion = 'domestic' | 'overseas';

/** 公開の料金ページ (site/)。プランを選ぶのはここだけで、窓口の中では選ばせない。`next` は申し込んだ後の戻り先。 */
function pricingHref(next: string) {
	return next === ACCOUNT_HOME ? PRICING_PATH : `${PRICING_PATH}?${new URLSearchParams({ next })}`;
}

/**
 * 選んだプランの最終確認の画面 (特定商取引法 12条の6)。支払いは次の Stripe の画面で行い、済んだら `next` へ戻る。
 * 条件はボタンより上に、畳まずに出す (→ docs/pro.md「売り方」)。
 */
export function confirmPage(
	lang: Lang,
	email: string,
	{ interval, region, next }: { interval: Interval; region: SaleRegion; next: string }
) {
	const t = messages[lang];
	const rows: [string, string][] = [
		[t.confirmPlanLabel, t.confirmPlan(interval)],
		[t.confirmPriceLabel, t.confirmPrice(interval)],
		[t.confirmRenewLabel, t.confirmRenew(interval)],
		[t.confirmPcsLabel, t.confirmPcs],
		[t.confirmPaymentLabel, t.confirmPayment[region]],
		[t.confirmCancelLabel, t.confirmCancel[region]]
	];
	return page(
		lang,
		t.confirmTitle,
		html`<h1>${t.confirmTitle}</h1>
			${orderList(rows)}
			<form method="post" action="${ACCOUNT}/buy">
				<p class="muted">${withLegalLinks(lang, t.buyConsent)}</p>
				<input type="hidden" name="next" value="${next}" />
				<input type="hidden" name="interval" value="${interval}" />
				<button>${t.confirmButton}</button>
			</form>
			<p><a href="${pricingHref(next)}">${t.changePlan}</a></p>
			${signedInAs(lang, email, next)}`
	);
}

/** 料金ページから申し込もうとしたが、もう Pro がある。 */
export function alreadyProPage(lang: Lang, next: string) {
	const t = messages[lang];
	return page(
		lang,
		t.buyTitle,
		html`<h1>${t.alreadyProTitle}</h1>
			<p>${t.alreadyPro}</p>
			<p><a href="${next}">${t.alreadyProContinue}</a></p>`
	);
}

/** 結ぶ画面で、Pro の無いアカウントに出す。料金ページから申し込めば、この画面へ戻る。 */
export function noProPage(lang: Lang, email: string, next: string, forSale: boolean) {
	const t = messages[lang];
	return page(
		lang,
		t.linkTitle,
		html`<h1>${t.noProHeading}</h1>
			${
				forSale
					? html`<p>${t.noProBuy}</p>
							<p><a class="action" href="${pricingHref(next)}">${t.seePricing}</a></p>`
					: ''
			}
			<p>${t.noPro}</p>
			${signedInAs(lang, email, next)}`
	);
}

/** 結ぶ WebLAV の名前・今の台数と上限を見せ、押したら結ぶ。 */
export function linkPage(
	lang: Lang,
	email: string,
	{
		request,
		name,
		pcName,
		relink,
		count,
		limit,
		next
	}: {
		request: string;
		name: string;
		/** 結ぶ PC の名前。読めなかった WebLAV からは空で届く。 */
		pcName: string;
		relink: boolean;
		count: number;
		limit: number;
		next: string;
	}
) {
	const t = messages[lang];
	const shown = installationName(name, pcName);
	return page(
		lang,
		t.linkTitle,
		html`<h1>${t.linkTitle}</h1>
			<p>${relink ? t.relinkLead(shown) : t.linkLead(shown)}</p>
			<p class="muted">${t.linkCount(count, limit)}</p>
			<p>${t.linkWarning}</p>
			<form method="post" action="${ACCOUNT}/link">
				<input type="hidden" name="r" value="${request}" />
				<input type="hidden" name="name" value="${name}" />
				<input type="hidden" name="pc" value="${pcName}" />
				<button>${t.linkButton}</button>
			</form>
			${signedInAs(lang, email, next)}`
	);
}

/** 一覧・結ぶ画面・メールに出す WebLAV の名前。サイト名だけでは見分けられないので、PC の名前を添える。 */
export function installationName(name: string, pcName: string): string {
	return pcName === '' ? name : `${name} (${pcName})`;
}

export type InstallationRow = {
	id: string;
	name: string;
	checkedAt: number | null;
	/** 外した行は、許可の期限 (それまで台数に数える)。外していなければ `null`。 */
	removedUntil: number | null;
	overLimit: boolean;
};

function installationList(lang: Lang, rows: InstallationRow[], next: string) {
	const t = messages[lang];
	if (rows.length === 0) return html`<p class="muted">${t.noInstallations}</p>`;
	return html`<ul>
		${rows.map(
			(row) =>
				html`<li>
					${t.installationLine(
						row.name,
						row.checkedAt === null ? undefined : formatDate(lang, row.checkedAt)
					)}
					${row.overLimit ? html`<span class="muted">(${t.overLimitShort})</span>` : ''}
					${
						row.removedUntil !== null
							? html`<p class="muted">${t.removedPending(formatDate(lang, row.removedUntil))}</p>`
							: html`<form method="post" action="${ACCOUNT}/installations/remove">
									<input type="hidden" name="id" value="${row.id}" />
									<input type="hidden" name="next" value="${next}" />
									<button class="secondary">${t.removeButton}</button>
								</form>`
					}
				</li>`
		)}
	</ul>`;
}

export function linkAtLimitPage(
	lang: Lang,
	email: string,
	limit: number,
	rows: InstallationRow[],
	next: string
) {
	const t = messages[lang];
	return page(
		lang,
		t.linkTitle,
		html`<h1>${t.linkAtLimitHeading}</h1>
			<p>${t.linkAtLimit(limit)}</p>
			${installationList(lang, rows, next)} ${signedInAs(lang, email, next)}`
	);
}

/** 結んだあと。ネットにつながらない WebLAV に打ち込む返しのコードを出す。 */
export function linkedPage(lang: Lang, code: string) {
	const t = messages[lang];
	return page(
		lang,
		t.linkedTitle,
		html`<h1>${t.linkedTitle}</h1>
			<p>${t.linkedLead}</p>
			<p>${t.linkedCodeLead}</p>
			<p class="code">${code}</p>
			<p><a href="${ACCOUNT_HOME}">${t.backToAccount}</a></p>`
	);
}

/** 外した証し (QR コード) を開いたところ。開いただけでは空けない (メールのリンクと同じく、先に開かれても使い切られないように)。 */
export function releasePage(lang: Lang, code: string) {
	const t = messages[lang];
	return page(
		lang,
		t.releaseTitle,
		html`<h1>${t.releaseTitle}</h1>
			<p>${t.releaseLead}</p>
			<form method="post" action="${ACCOUNT}/release">
				<label for="c">${t.releaseCodeLabel}</label>
				<input
					id="c"
					name="c"
					value="${code}"
					autocomplete="off"
					autocapitalize="characters"
					required
				/>
				<button>${t.releaseButton}</button>
			</form>`
	);
}

export function homePage(
	lang: Lang,
	email: string,
	{
		plans,
		billing,
		limit,
		installations,
		forSale,
		switchable
	}: {
		plans: { plan: Plan; paidThrough: number }[];
		billing: boolean;
		/** 月額と年額を切り替えられる、個人向けの Stripe のサブスクがある。 */
		switchable: boolean;
		limit: number;
		installations: InstallationRow[];
		forSale: boolean;
	}
) {
	const t = messages[lang];
	const next = ACCOUNT_HOME;
	return page(
		lang,
		t.accountTitle,
		html`<h1>${t.accountTitle}</h1>
			${
				plans.length > 0
					? html`<ul>
							${plans.map(
								(p) => html`<li>${t.planUntil(p.plan, formatDate(lang, p.paidThrough))}</li>`
							)}
						</ul>`
					: html`<p>${t.noPlans}</p>`
			}
			${
				billing
					? html`<form method="post" action="${ACCOUNT}/billing">
							<button class="secondary">${t.manageBilling}</button>
						</form>`
					: ''
			}
			${switchable ? html`<p><a href="${PLAN_PATH}">${t.planSwitchTitle}</a></p>` : ''}
			${plans.length > 0 ? html`<p><a href="${TRANSFER_PATH}">${t.transferTitle}</a></p>` : ''}
			${
				plans.length === 0 && forSale
					? html`<p><a class="action" href="${pricingHref(next)}">${t.seePricing}</a></p>`
					: ''
			}
			<h2>${t.installationsHeading}</h2>
			${installations.some((i) => i.overLimit) ? html`<p role="alert">${t.overLimit(limit)}</p>` : ''}
			${installationList(lang, installations, next)} ${signedInAs(lang, email, next)}`
	);
}

/**
 * 支払いから戻った先で、Pro が付くのを待つ画面。`autoRetry` の間は数秒おきに開き直す。
 * 待っても付かなければ、確かめ直す手段を出す (買えたか分からないまま買い直さないように)。
 */
export function checkingPurchasePage(lang: Lang, retryUrl: string, autoRetry: boolean) {
	const t = messages[lang];
	return page(
		lang,
		t.buyTitle,
		html`${autoRetry ? html`<meta http-equiv="refresh" content="3;url=${retryUrl}" />` : ''}
			<h1>${t.buyTitle}</h1>
			<p>${autoRetry ? t.checkingPurchase : t.purchaseNotYet}</p>
			${autoRetry ? '' : html`<p><a href="${retryUrl}">${t.checkAgain}</a></p>`}`
	);
}

export function transferPage(lang: Lang, email: string, error?: string) {
	const t = messages[lang];
	return page(
		lang,
		t.transferTitle,
		html`<h1>${t.transferTitle}</h1>
			<p>${t.transferLead}</p>
			${error ? html`<p role="alert">${error}</p>` : ''}
			<form method="post" action="${TRANSFER_PATH}">
				<label for="email">${t.transferTo}</label>
				<input id="email" name="email" type="email" autocomplete="off" required />
				<button>${t.next}</button>
			</form>
			${signedInAs(lang, email, TRANSFER_PATH)}`
	);
}

export function transferredPage(lang: Lang, to: string, notified: boolean) {
	const t = messages[lang];
	const message = notified ? t.transferred(to) : t.transferredNotNotified(to);
	return messagePage(lang, t.transferredTitle, message, true);
}

export function transferConfirmPage(lang: Lang, email: string, to: string, plans: Plan[]) {
	const t = messages[lang];
	return page(
		lang,
		t.transferTitle,
		html`<h1>${t.transferTitle}</h1>
			<p>${t.transferConfirm(t.plans(plans), to)}</p>
			<p class="muted">${t.transferWarning}</p>
			<form method="post" action="${TRANSFER_PATH}">
				<input type="hidden" name="email" value="${to}" />
				<input type="hidden" name="confirm" value="1" />
				<button>${t.transferButton}</button>
			</form>
			<p><a href="${TRANSFER_PATH}">${t.transferBack}</a></p>
			${signedInAs(lang, email, TRANSFER_PATH)}`
	);
}

/** 月額と年額を切り替える画面の中身。 */
export type PlanSwitch =
	| {
			kind: 'toYearly';
			region: SaleRegion;
			/** 今日払う額と差し引く額 (書式を整えたもの)、年額の次の更新。 */
			total: string;
			credit: string;
			renewsAt: number;
			/** 日割りを計算した時刻。切り替えるときにも同じ時刻で計算させる。 */
			at: number;
	  }
	| { kind: 'toMonthly'; region: SaleRegion; switchAt: number }
	| { kind: 'reserved'; switchAt: number }
	| { kind: 'canceled'; until: number };

/**
 * 月額と年額を切り替える画面。年額へは今すぐ払うので、申し込みの最終確認の画面と同じ事項をボタンより上に出す
 * (特定商取引法 12条の6)。月額へは年額の期間の終わりに切り替わるよう予約する。
 */
export function planSwitchPage(lang: Lang, email: string, view: PlanSwitch, notice?: string) {
	const t = messages[lang];
	const form = (action: string, button: string, extra = html``) =>
		html`<form method="post" action="${PLAN_PATH}">
			${extra}
			<input type="hidden" name="action" value="${action}" />
			<button>${button}</button>
		</form>`;
	let body;
	switch (view.kind) {
		case 'toYearly':
			body = html`<p>${t.planCurrent('month')}</p>
				${orderList([
					[t.confirmPlanLabel, t.confirmPlan('year')],
					[t.confirmPriceLabel, t.confirmPrice('year')],
					[t.switchTodayLabel, t.switchToday(view.total, view.credit)],
					[t.confirmRenewLabel, t.switchYearlyRenew(formatDate(lang, view.renewsAt))],
					[t.confirmPcsLabel, t.confirmPcs],
					[t.confirmPaymentLabel, t.switchPayment[view.region]],
					[t.confirmCancelLabel, t.confirmCancel[view.region]]
				])}
				${form(
					'year',
					t.switchYearlyButton,
					html`<p class="muted">${withLegalLinks(lang, t.buyConsent)}</p>
						<input type="hidden" name="at" value="${view.at}" />`
				)}`;
			break;
		case 'toMonthly': {
			const date = formatDate(lang, view.switchAt);
			body = html`<p>${t.planCurrent('year')}</p>
				<p>${t.switchMonthlyLead(date)}</p>
				${orderList([
					[t.confirmPlanLabel, t.confirmPlan('month')],
					[t.confirmPriceLabel, t.confirmPrice('month')],
					[t.switchDateLabel, date],
					[t.confirmRenewLabel, t.confirmRenew('month')],
					[t.confirmCancelLabel, t.confirmCancel[view.region]]
				])}
				${form(
					'month',
					t.switchMonthlyButton(date),
					html`<p class="muted">${withLegalLinks(lang, t.buyConsent)}</p>`
				)}`;
			break;
		}
		case 'reserved':
			body = html`<p>${t.planReserved(formatDate(lang, view.switchAt))}</p>
				${form('release', t.switchReleaseButton)}`;
			break;
		case 'canceled':
			body = html`<p>${t.planCanceled(formatDate(lang, view.until))}</p>`;
			break;
	}
	return page(
		lang,
		t.planSwitchTitle,
		html`<h1>${t.planSwitchTitle}</h1>
			${notice ? html`<p role="alert">${notice}</p>` : ''} ${body}
			<p><a href="${ACCOUNT_HOME}">${t.backToAccount}</a></p>
			${signedInAs(lang, email, PLAN_PATH)}`
	);
}
