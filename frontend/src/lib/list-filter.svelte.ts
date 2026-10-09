import { getContext, setContext, untrack } from 'svelte';
import { replaceState } from '$app/navigation';
import { page } from '$app/state';
import { withQuery } from '$lib/href';
import { matchFilter, parseFilterTerms } from '$lib/list-filter';

/** 絞り込みの語を持つ URL クエリのキー。アーカイブの軸の名前としては使えない (→ docs/ui.md「一覧の絞り込み」)。 */
export const LIST_FILTER_QUERY = 'filter';

const CONTEXT_KEY = Symbol('list-filter');

/**
 * 一覧のページ内の絞り込み (→ docs/ui.md「一覧の絞り込み」)。ページが作って context に置き、
 * 一覧の部品が読んで行を絞る。置かない画面 (検索の結果など) の一覧は絞らない。
 */
export class ListFilterState {
	value = $state('');
	terms = $derived(parseFilterTerms(this.value));
	/** 最後に URL へ書いた語。これと違う語が URL に来たら、戻る・進むなど外で変わったもの。 */
	#written = '';

	constructor() {
		$effect.pre(() => {
			const fromUrl = page.url.searchParams.get(LIST_FILTER_QUERY) ?? '';
			untrack(() => {
				if (fromUrl === this.#written) return;
				this.value = fromUrl;
				this.#written = fromUrl;
			});
		});
	}

	get active(): boolean {
		return this.terms.length > 0;
	}

	/** 当たった文字の位置。語が無ければ空 (全部残す)。当たらなければ `null`。 */
	match(text: string): number[] | null {
		return this.active ? matchFilter(text, this.terms) : [];
	}

	/**
	 * 語を変え、URL にも写す。読み込み直さないよう、履歴は shallow routing で置き換える。
	 * 打つたびに戻るの段を増やさないため。
	 */
	set(next: string) {
		this.value = next;
		const query = next.trim() === '' ? '' : next;
		this.#written = query;
		const params = Object.fromEntries(page.url.searchParams);
		delete params[LIST_FILTER_QUERY];
		const href = withQuery(
			page.url.pathname,
			query === '' ? params : { ...params, [LIST_FILTER_QUERY]: query }
		);
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- 今の URL のパスをそのまま使う (→ AGENTS.md「コードの規約」)
		replaceState(href, page.state);
	}

	/** ほかのクエリ (並び順など) へ移る href に、今の語を引き継ぐための項目。 */
	get query(): Record<string, string> {
		return this.value.trim() === '' ? {} : { [LIST_FILTER_QUERY]: this.value };
	}
}

export function provideListFilter(state: ListFilterState) {
	setContext(CONTEXT_KEY, state);
}

export function listFilter(): ListFilterState | undefined {
	return getContext<ListFilterState | undefined>(CONTEXT_KEY);
}
