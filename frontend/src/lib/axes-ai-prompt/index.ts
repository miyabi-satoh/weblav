/**
 * アーカイブの軸の定義を外部の AI に考えてもらうプロンプトと、その答えの読み取り
 * (→ docs/archive.md「軸の定義を外部の AI で作る」)。
 */
import { getLocale } from '$lib/paraglide/runtime';
import type { components } from '$lib/api/schema';
import ja from './ja.md?raw';
import en from './en.md?raw';

type DirLevelSummary = components['schemas']['DirLevelSummary'];
type FilenameWord = components['schemas']['FilenameWord'];
export type ImportAxesRequest = components['schemas']['ImportAxesRequest'];

// ADR: プロンプトは画面の文言 (Paraglide) でなく、言語ごとの Markdown に置く。
// JSON の例やテンプレートの `{軸名}` の波かっこが、Paraglide の差し込みの記法とぶつかるため。
const TEMPLATES: Record<string, string> = { ja, en };

/** プロンプトに載せる語の数。よく出る語で軸の見当が付けば足り、多いと貼り付けが重くなる。 */
const WORD_LIMIT = 40;
/** プロンプトに載せるパスの数。フォルダーの構造とファイル名の付け方が分かれば足りる。 */
const PATH_LIMIT = 150;

/** 全体から間を空けて `limit` 件を選ぶ。並びの先頭だけだと、最初のフォルダーのファイルに偏るため。 */
export function spread<T>(items: T[], limit: number): T[] {
	if (items.length <= limit) return items;
	const step = items.length / limit;
	return Array.from({ length: limit }, (_, i) => items[Math.floor(i * step)]);
}

export function buildAxesPrompt({
	itemCount,
	levels,
	words,
	relPaths
}: {
	itemCount: number;
	levels: DirLevelSummary[];
	words: FilenameWord[];
	relPaths: string[];
}): string {
	const template = TEMPLATES[getLocale()] ?? en;
	const lines = (values: string[]) => (values.length > 0 ? values.join('\n') : '-');
	const replacements: Record<string, string> = {
		ITEM_COUNT: String(itemCount),
		LEVELS: lines(
			levels.map(
				(level) =>
					`- ${level.level}: ${level.valueCount} (${level.samples.map((s) => s.value).join(', ')})`
			)
		),
		WORDS: lines(
			words
				.filter((word) => !word.hiddenByDefault)
				.slice(0, WORD_LIMIT)
				.map((word) => `- ${word.word}: ${word.count}`)
		),
		PATHS: lines(spread([...relPaths].sort(), PATH_LIMIT).map((path) => `- ${path}`))
	};
	return template.replace(/\{\{(\w+)\}\}/g, (match, key: string) => replacements[key] ?? match);
}

/** AI の答えを読めなかった理由。 */
export type ParseFailure = 'notJson' | 'notAxes';

/**
 * AI の答えから JSON を取り出す。AI は前後に説明やコードブロックの囲みを付けがちなので、
 * 囲みがあればその中を、無ければ最初の `{` から最後の `}` までを読む。囲みを先に見るのは、
 * 説明の中に `{年度}` のようなテンプレートの例が書かれることがあるため。
 * 形の細かい確かめはサーバーに任せる (保存と同じ決まりで確かめるため)。
 */
export function parseAxesAnswer(
	text: string
): { ok: true; value: ImportAxesRequest } | { ok: false; reason: ParseFailure } {
	const answer = /```(?:json)?\s*\n([\s\S]*?)```/.exec(text)?.[1] ?? text;
	const start = answer.indexOf('{');
	const end = answer.lastIndexOf('}');
	if (start < 0 || end < start) return { ok: false, reason: 'notJson' };
	let value: unknown;
	try {
		value = JSON.parse(answer.slice(start, end + 1));
	} catch {
		return { ok: false, reason: 'notJson' };
	}
	if (
		typeof value !== 'object' ||
		value === null ||
		!Array.isArray((value as { axes?: unknown }).axes)
	) {
		return { ok: false, reason: 'notAxes' };
	}
	return { ok: true, value: value as ImportAxesRequest };
}
