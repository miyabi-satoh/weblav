/**
 * 名前の語で絞り込む、サーバーで区切る一覧の取り直し (→ docs/ui.md「一覧の絞り込み」)。
 * ファイルのタブと対象外のタブで同じにする。
 *
 * ページの読み込みは画面の URL の語しか知らない。語は shallow routing で履歴に写すので、読み込みは走らず、
 * 読み込み直し (`refreshAll`) も打つ前の語で取る。語が読み込みと違う間はここで取り直した結果を出す。
 * 取り直している間は前の行を出したままにする。読み込みの結果 (絞っていない行) を一瞬出さないため。
 */
import { refreshAll } from '$app/navigation';
import { untrack } from 'svelte';

/** 打ち止めてから送るまでの間。 */
const DEBOUNCE_MS = 300;

type Options<D, V> = {
	data: () => D;
	viewOf: (data: D) => V;
	/** 打っている語。 */
	typed: () => string;
	/** その読み込みの条件に語を足して取る。取れなければ `undefined` (理由は呼び出し側が出す)。 */
	fetch: (data: D, name: string) => Promise<V | undefined>;
	/** 語のほかの条件 (軸・並び順) を表す値。同じ間は、取り直しても見ていた所へ戻す。 */
	conditions?: (data: D) => string;
	/** 見ていた所。`restore` に渡す。 */
	position?: () => number;
	restore?: (position: number) => void;
};

export class NameFilteredView<D extends { nameFilter: string }, V> {
	/** 一覧に効いている語。 */
	applied = $state('');
	#result = $state.raw<{ data: D; name: string; view: V } | null>(null);
	/** 最後に出した結果。画面には出さず、取り直している間の代わりに読むだけなので、`$state` にしない。 */
	#last: V | undefined;
	#options: Options<D, V>;
	#refreshing = false;

	/** 読み込みの結果をそのまま出せるか。 */
	get fromData(): boolean {
		return this.applied === this.#options.data().nameFilter;
	}

	view = $derived.by(() => {
		const data = this.#options.data();
		const result = this.#result;
		let view: V;
		if (this.applied === data.nameFilter) view = this.#options.viewOf(data);
		else if (result?.data === data && result.name === this.applied) view = result.view;
		else view = this.#last ?? this.#options.viewOf(data);
		this.#last = view;
		return view;
	});

	constructor(options: Options<D, V>) {
		this.#options = options;
		this.applied = untrack(() => options.data().nameFilter);

		$effect(() => {
			const typed = options.typed().trim();
			if (typed === this.applied.trim()) return;
			const timer = setTimeout(() => {
				this.applied = typed;
				if (!this.fromData) void this.refetch(false);
			}, DEBOUNCE_MS);
			return () => clearTimeout(timer);
		});

		// 読み込みが替わったら、打っている語で取り直す (軸・並び順を選び直した、再スキャンが終わった など)。
		let previous = untrack(() => options.data());
		$effect(() => {
			const current = options.data();
			if (current === previous) return;
			const before = previous;
			previous = current;
			untrack(() => {
				if (this.#refreshing) return;
				this.applied = options.typed().trim();
				if (this.fromData) return;
				const same = options.conditions?.(before) === options.conditions?.(current);
				void this.refetch(same);
			});
		});
	}

	/** 今の条件と語で、最初の区切りから取り直す。 */
	async refetch(keepPosition: boolean): Promise<void> {
		const position = keepPosition ? (this.#options.position?.() ?? 0) : 0;
		const data = this.#options.data();
		const name = this.applied;
		const view = await this.#options.fetch(data, name);
		// 待つ間にページを移った・別の語を打ったなら、古い結果を出さない。
		if (view === undefined || data !== this.#options.data() || name !== this.applied) return;
		this.#result = { data, name, view };
		if (position > 0) this.#options.restore?.(position);
	}

	/** 書き換えた後に、見出しの件数ごと取り直す。 */
	async refresh(): Promise<void> {
		this.#refreshing = true;
		try {
			await refreshAll();
			if (!this.fromData) await this.refetch(true);
		} finally {
			this.#refreshing = false;
		}
	}
}
