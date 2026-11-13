// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		interface PageState {
			/** 一覧のページ内の絞り込みの語 (→ $lib/list-filter.svelte.ts)。 */
			listFilter?: string;
			/** ページ内で開いているものの key。`null` は開いていない (→ $lib/preview-url.svelte.ts)。 */
			preview?: string | null;
			/** この履歴の段は、ページ内で開いたときに足したもの。閉じるときは戻る。 */
			previewPushed?: boolean;
		}
		// interface Platform {}
	}
}

export {};
