import { withQuery } from '$lib/href';
import { getLocale } from '$lib/paraglide/runtime';

/** サーバーが返す量の単位。1回の購入で付く量を 1,000,000 とした整数 (→ docs/ai.md「窓口の呼び出しと、使った量の記録」)。 */
const USED_PER_PURCHASE = 1_000_000;

/** 割合を小数1桁まで、画面の表示言語の書式で書く。 */
export function formatPercent(percent: number): string {
	return `${percent.toLocaleString(getLocale(), { maximumFractionDigits: 1 })}%`;
}

/**
 * 使った量を、1回の購入で付く量に対する割合に直す。
 * 小数1桁に丸めると 0 になる量は、使っていないのと見分けられるよう `belowMinimum` にする。
 */
export function usagePercent(used: number): { belowMinimum: boolean; text: string } {
	const percent = (used / USED_PER_PURCHASE) * 100;
	if (percent > 0 && percent < 0.1) return { belowMinimum: true, text: formatPercent(0.1) };
	return { belowMinimum: false, text: formatPercent(percent) };
}

/**
 * WebLAV のアカウントの画面の URL に、この画面の表示言語を付ける。
 * 開いた先が、ブラウザーの言語でなく今の画面と同じ言語で出るようにするため。
 */
export function accountPageHref(url: string): string {
	const [base, query = ''] = url.split('?');
	return withQuery(base, { ...Object.fromEntries(new URLSearchParams(query)), lang: getLocale() });
}
