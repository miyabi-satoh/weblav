/**
 * ビューアーで開いたファイルの、コンテンツの中の場所。Markdown のプレビューが、相対パスの画像とリンクを
 * このファイルの場所から解決するのに使う (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
 */
export type ContentLocation = {
	/** コンテンツの登録先からの相対パス (`/` 区切り)。 */
	path: string;
	/** 同じコンテンツの中の、`path` (登録先からの相対パス) にあるファイルの URL。 */
	src: (path: string) => string;
};

/** 相対パスとして解決しない参照。スキームのあるもの・`/` や `#` で始まるもの。 */
const NOT_RELATIVE = /^([a-z][a-z0-9+.-]*:|\/|#)/i;

/**
 * `from` のファイルから見た `href` を、登録先からの相対パスにする。相対パスでないもの・登録先の外を指すものは `undefined`。
 * `/` で始まるものを登録先からと読まないのは、リポジトリの根から書かれていることが多く、登録先と一致しないため。
 */
export function resolveRelativePath(from: string, href: string): string | undefined {
	if (NOT_RELATIVE.test(href)) return undefined;
	const target = href.split(/[?#]/, 1)[0];
	if (target === '') return undefined;
	const segments = from.split('/').slice(0, -1);
	for (const raw of target.split('/')) {
		let segment: string;
		try {
			segment = decodeURIComponent(raw);
		} catch {
			return undefined;
		}
		if (segment === '' || segment === '.') continue;
		if (segment === '..') {
			if (segments.length === 0) return undefined;
			segments.pop();
		} else {
			segments.push(segment);
		}
	}
	return segments.length === 0 ? undefined : segments.join('/');
}
