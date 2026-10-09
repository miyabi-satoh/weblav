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
		}
		// interface Platform {}
	}
}

export {};
