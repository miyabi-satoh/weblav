import { client } from '$lib/api/client';
import { getLocale } from '$lib/paraglide/runtime';

/** 関心の言葉の行の数と、1行の長さ (UTF-16 の単位) の上限。サーバーの検証 (`src/api/ai_links.rs`) と同じ値。 */
export const MAX_INTERESTS = 5;
export const MAX_INTEREST_UNITS = 200;

/** 関心の言葉を、空の行を捨てた行の並びにする。 */
export function interestLines(text: string): string[] {
	return text
		.split('\n')
		.map((line) => line.trim())
		.filter((line) => line !== '');
}

/**
 * グループのリンクを AI に集め直してもらう (→ docs/ai.md「AI が集めるリンク」)。
 * 画面の表示言語のページを引くよう、表示言語を添える。
 */
export function refreshAiLinks(id: number) {
	return client.POST('/api/v1/contents/{id}/ai-links/refresh', {
		params: { path: { id } },
		body: { language: getLocale() }
	});
}
