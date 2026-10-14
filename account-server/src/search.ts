/**
 * Web の検索の中継 (→ docs/account-server.md「検索の中継」)。AI が集めるリンクの候補を引く。
 * 検索の業者 (Perplexity の Search API) を呼ぶのはこのファイルだけにし、差し替えるときにここだけを直せるようにする。
 */
import type { Rates } from './pricing';

const SEARCH_URL = 'https://api.perplexity.ai/search';
/** 1回に渡せる語の数。業者が1リクエストとして数える上限。 */
const MAX_QUERIES = 5;
/** クエリの長さの上限。AI が作る短いクエリに足り、文章を丸ごと送るのには使わせない。 */
const MAX_QUERY_CHARS = 200;
/** 返る候補の数。業者の上限。 */
const MAX_RESULTS = 20;
/** 抜粋の長さ。AI が集めたいものに合うかを選ぶのに足りる短さにし、ページの中身の写しにしない。 */
const SNIPPET_TOKENS = 128;
const TIMEOUT_MS = 20_000;

/** WebLAV が送る中身。 */
export type SearchRequest = {
	queries: string[];
	/** 国 (ISO 3166-1 alpha-2) と言語 (ISO 639-1)。WebLAV の表示言語から決める。 */
	country?: string;
	language?: string;
	/** 新しさ。1か月以下は、量産の記事が多く混じるので受けない。 */
	recency?: 'year';
};

/** 受けた本文を `SearchRequest` として読む。形が違えば `undefined`。 */
export function parseSearch(body: unknown): SearchRequest | undefined {
	if (typeof body !== 'object' || body === null) return undefined;
	const { queries, country, language, recency } = body as Record<string, unknown>;
	if (
		!Array.isArray(queries) ||
		queries.length < 1 ||
		queries.length > MAX_QUERIES ||
		!queries.every((q) => typeof q === 'string' && q.trim() !== '' && q.length <= MAX_QUERY_CHARS)
	) {
		return undefined;
	}
	if (country !== undefined && (typeof country !== 'string' || !/^[A-Z]{2}$/.test(country))) {
		return undefined;
	}
	if (language !== undefined && (typeof language !== 'string' || !/^[a-z]{2}$/.test(language))) {
		return undefined;
	}
	if (recency !== undefined && recency !== 'year') return undefined;
	return {
		queries: queries as string[],
		...(country === undefined ? {} : { country }),
		...(language === undefined ? {} : { language }),
		...(recency === undefined ? {} : { recency })
	};
}

/** 候補の1件。`snippet` は選ぶためだけに使い、WebLAV は保存しない。 */
export type SearchResult = { title: string; url: string; date?: string; snippet: string };

export type Searched =
	{ ok: true; results: SearchResult[]; cost: number } | { ok: false; status: number };

/** 1リクエストの原価 (milli_yen、切り上げ)。業者は成功したリクエストだけを数える。 */
export function searchCost(rates: Rates): number {
	return Math.ceil(rates.searchUsdPerRequest * rates.usdJpy * 1000);
}

/** 検索する。届かなかったときは投げる (引かない)。 */
export async function search(env: Env, request: SearchRequest, rates: Rates): Promise<Searched> {
	const res = await fetch(SEARCH_URL, {
		method: 'POST',
		headers: {
			authorization: `Bearer ${env.PERPLEXITY_API_KEY ?? ''}`,
			'content-type': 'application/json'
		},
		body: JSON.stringify({
			query: request.queries,
			// 速い版。ふつうの版と返るページがほぼ同じで、単価が低い。
			search_type: 'fast',
			max_results: MAX_RESULTS,
			max_tokens_per_page: SNIPPET_TOKENS,
			...(request.country === undefined ? {} : { country: request.country }),
			...(request.language === undefined ? {} : { search_language_filter: [request.language] }),
			...(request.recency === undefined ? {} : { search_recency_filter: request.recency })
		}),
		signal: AbortSignal.timeout(TIMEOUT_MS)
	});
	if (!res.ok) {
		// 語を含みうるので本文は残さず、状態だけを残す。
		console.error('Search responded', res.status);
		return { ok: false, status: res.status };
	}
	const body = await res.json<{ results?: unknown }>().catch(() => ({ results: undefined }));
	const results: SearchResult[] = [];
	for (const item of Array.isArray(body.results) ? body.results : []) {
		if (typeof item !== 'object' || item === null) continue;
		const { title, url, date, last_updated: updated, snippet } = item as Record<string, unknown>;
		if (typeof url !== 'string' || !/^https?:\/\//.test(url)) continue;
		const day = typeof date === 'string' ? date : typeof updated === 'string' ? updated : undefined;
		results.push({
			title: typeof title === 'string' ? title : '',
			url,
			...(day === undefined ? {} : { date: day }),
			snippet: typeof snippet === 'string' ? snippet : ''
		});
	}
	return { ok: true, results, cost: searchCost(rates) };
}
