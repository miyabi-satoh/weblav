import { client } from '$lib/api/client';
import { getLocale } from '$lib/paraglide/runtime';

/** 検索キーワードに書けるクエリ (1行に1つ) の数と、1行の長さ (UTF-16 の単位) の上限。サーバーの検証 (`src/api/ai_links.rs`) と同じ値。 */
export const MAX_QUERIES = 5;
export const MAX_QUERY_UNITS = 200;

/** 検索キーワードの行のうち、クエリになるもの。サーバー (`query_lines`) と同じく、空の行と重なった行を除く。 */
export function queryLines(text: string): string[] {
	const lines = text.split('\n').map((line) => line.split(/\s+/).filter(Boolean).join(' '));
	return [...new Set(lines.filter((line) => line !== ''))];
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

/**
 * 時間が来ていれば、グループのリンクを自動で集め直してもらう (→ docs/ai.md「自動の更新」)。
 * 見ている人が頼んだ更新ではないので、サーバーは失敗しても今のリンクを返す。
 */
export function autoRefreshAiLinks(id: number) {
	return client.POST('/api/v1/contents/{id}/ai-links/auto-refresh', {
		params: { path: { id } },
		body: { language: getLocale() }
	});
}

/** 集めたリンクのサイト。サーバー (`site_of`) と同じく、`www.` を外したホスト名。 */
export function aiLinkSite(url: string): string | null {
	try {
		const host = new URL(url).hostname.toLowerCase();
		return host.startsWith('www.') ? host.slice(4) : host;
	} catch {
		return null;
	}
}
