import { getContext, setContext, untrack } from 'svelte';
import { beforeNavigate, replaceState } from '$app/navigation';
import { page } from '$app/state';
import { withQuery } from '$lib/href';
import { LIST_FILTER_QUERY, matchFilter, parseFilterTerms } from '$lib/list-filter';

const CONTEXT_KEY = Symbol('list-filter');

/**
 * 打ち止めてから URL に写すまでの間。Safari は短い間に何度も履歴を書き換えると例外を投げ、
 * IME の変換中も入力が届くため、打つたびには写さない。行はその場で絞る。
 */
const URL_WRITE_DELAY_MS = 300;

/**
 * 一覧のページ内の絞り込み (→ docs/ui.md「一覧の絞り込み」)。ページが作って context に置き、
 * 一覧の部品が読んで行を絞る。置かない画面 (検索の結果など) の一覧は絞らない。
 */
export class ListFilterState {
	value = $state('');
	terms = $derived(parseFilterTerms(this.value));
	/** 前に読んだ、外から来た語。変わったときだけ受け取る (同じ語での読み込み直しで、打った値を戻さないため)。 */
	#seen: string | undefined;
	#pendingWrite: ReturnType<typeof setTimeout> | undefined;

	constructor() {
		// shallow routing の replaceState は page.url を変えないので、語は page.state にも持ち、そちらを先に読む。
		// 戻る・進むでは page.state が戻り、読み込み直し (再読み込み・共有されたリンク) では URL から読む。
		$effect.pre(() => {
			const incoming = page.state.listFilter ?? page.url.searchParams.get(LIST_FILTER_QUERY) ?? '';
			untrack(() => {
				if (incoming === this.#seen) return;
				this.#seen = incoming;
				this.value = incoming;
			});
		});
		// 書きかけの URL を、移った先の画面に書かないよう取り消す。
		beforeNavigate(() => clearTimeout(this.#pendingWrite));
		$effect(() => () => clearTimeout(this.#pendingWrite));
	}

	get active(): boolean {
		return this.terms.length > 0;
	}

	/** 当たった文字の位置。語が無ければ空 (全部残す)。当たらなければ `null`。 */
	match(text: string): number[] | null {
		return this.active ? matchFilter(text, this.terms) : [];
	}

	/** タイトルで当たる行だけを、並びを変えずに返す。 */
	apply<T>(rows: T[], titleOf: (row: T) => string): T[] {
		return this.active ? rows.filter((row) => this.match(titleOf(row)) !== null) : rows;
	}

	/**
	 * 語を変え、間を置いて URL にも写す。読み込み直さないよう、履歴は shallow routing で置き換える。
	 * 打つたびに戻るの段を増やさないため。
	 */
	set(next: string) {
		this.value = next;
		clearTimeout(this.#pendingWrite);
		this.#pendingWrite = setTimeout(() => this.#write(), URL_WRITE_DELAY_MS);
	}

	#write() {
		const query = this.value.trim() === '' ? '' : this.value;
		this.#seen = query;
		const params = Object.fromEntries(page.url.searchParams);
		delete params[LIST_FILTER_QUERY];
		const href = withQuery(
			page.url.pathname,
			query === '' ? params : { ...params, [LIST_FILTER_QUERY]: query }
		);
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- 今の URL のパスをそのまま使う (→ AGENTS.md「コードの規約」)
		replaceState(href, { ...page.state, listFilter: query });
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
