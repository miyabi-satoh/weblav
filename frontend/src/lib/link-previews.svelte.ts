import { client } from '$lib/api/client';
import type { components } from '$lib/api/schema';

type LinkPreview = components['schemas']['LinkPreview'];
type RefreshLinkPreviewsRequest = components['schemas']['RefreshLinkPreviewsRequest'];

/**
 * 一覧に出したリンクのカードを、サーバーに取り直してもらった情報で差し替える (→ docs/ui.md「リンクのカード」)。
 * 一覧は覚えている情報で先に出しているので、取り直せなければそのままにする。
 * 頼む中身が変わったら頼み直し、前の答えは捨てる。コンポーネントの初期化の中で作ること (`$effect` を使うため)。
 */
export class RefreshedLinkPreviews {
	#previews = $state<Record<string, LinkPreview>>({});

	/** @param request 頼む中身。リンクが無ければ `undefined` を返し、頼まない。 */
	constructor(request: () => RefreshLinkPreviewsRequest | undefined) {
		$effect(() => {
			const body = request();
			if (!body) return;
			let cancelled = false;
			client
				.POST('/api/v1/link-previews/refresh', { body })
				.then(({ data }) => {
					if (cancelled) return;
					this.#previews = Object.fromEntries(
						(data ?? []).map((entry) => [entry.url, entry.preview])
					);
				})
				.catch(() => {
					if (!cancelled) this.#previews = {};
				});
			return () => {
				cancelled = true;
			};
		});
	}

	/** `url` の取り直した情報。まだ答えが無ければ `undefined`。 */
	get(url: string): LinkPreview | undefined {
		return this.#previews[url];
	}
}
