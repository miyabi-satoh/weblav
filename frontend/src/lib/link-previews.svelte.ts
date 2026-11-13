import { SvelteMap } from 'svelte/reactivity';
import { client } from '#lib/api/client.ts';
import type { components } from '#lib/api/schema.js';
import { readNdjson } from '#lib/ndjson.ts';

type LinkPreview = components['schemas']['LinkPreview'];
type RefreshedLinkPreview = components['schemas']['RefreshedLinkPreview'];
type RefreshLinkPreviewsRequest = components['schemas']['RefreshLinkPreviewsRequest'];

/**
 * 一覧に出したリンクのカードを、サーバーに取り直してもらった情報で差し替える (→ docs/ui.md「リンクのカード」)。
 * 一覧は覚えている情報で先に出しているので、取り直せなければそのままにする。
 * サーバーは確かめ終えた順に1件ずつ返すので、届いたカードから差し替える。
 * 頼む中身が変わったら頼み直し、前の答えは捨てる。コンポーネントの初期化の中で作ること (`$effect` を使うため)。
 */
export class RefreshedLinkPreviews {
	#previews = new SvelteMap<string, LinkPreview>();

	/** @param request 頼む中身。リンクが無ければ `undefined` を返し、頼まない。 */
	constructor(request: () => RefreshLinkPreviewsRequest | undefined) {
		$effect(() => {
			const body = request();
			this.#previews.clear();
			if (!body) return;
			let cancelled = false;
			const controller = new AbortController();
			client
				.POST('/api/v1/link-previews/refresh', {
					body,
					parseAs: 'stream',
					signal: controller.signal
				})
				.then(({ data }) => {
					if (!data || cancelled) return;
					return readNdjson<RefreshedLinkPreview>(data, (entry) => {
						if (!cancelled) this.#previews.set(entry.url, entry.preview);
					});
				})
				.catch(() => {
					// 取り直せなかった分は、覚えている情報のまま出しておけば足りる。
				});
			return () => {
				cancelled = true;
				controller.abort();
			};
		});
	}

	/** `url` の取り直した情報。まだ答えが無ければ `undefined`。 */
	get(url: string): LinkPreview | undefined {
		return this.#previews.get(url);
	}
}
