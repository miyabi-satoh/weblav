/**
 * アーカイブの一覧を区切って取りに行き、手元に持つ (→ docs/archive.md「画面が見える辺りを取りに行く」)。
 *
 * 最初の要求で全体の件数と先頭の区切りを受け取り、残りは見えてきた区切りだけを取りに行く。
 * サーバーは最初の要求で絞って並べた結果を持ち (`snapshot`)、続きはその結果を位置で引くので、
 * 途中でほかの人が公開を切り替えても、取った行と後で取る行はずれない。
 */

import { SvelteMap, SvelteSet } from 'svelte/reactivity';

/** 1回に取る行の数。サーバーの上限 (`archive_items::PAGE_LIMIT`) と同じ。 */
export const ARCHIVE_CHUNK_SIZE = 500;

/**
 * 手元に残す区切りの数。全部残すと、終わりまでスクロールしたスマートフォンで今と同じ数百MBに戻るため。
 * 1万行あれば、見えている所の前後を行き来しても取り直さずに済む。
 */
const MAX_CHUNKS = 20;

/** 区切りの続きの応答。結果が捨てられていたら `expired`。 */
export type ChunkResult<T> = { items: (T | null)[]; stale: boolean } | { expired: true };

export type ChunkFetch<T> = (
	snapshot: string,
	offset: number,
	signal: AbortSignal
) => Promise<ChunkResult<T>>;

export class ArchivePagedList<T> {
	readonly snapshot: string;
	readonly total: number;
	/** 結果を作った後に、索引か公開・非公開が変わった。画面は取り直しを出す。 */
	stale = $state(false);
	/** 結果が捨てられた (しばらく使わなかった)。画面は最初から取り直す。 */
	expired = $state(false);
	/** 取れなかった区切りの番号。その場所に押し直しを出す。 */
	readonly failed = new SvelteSet<number>();

	#chunks = new SvelteMap<number, (T | null)[]>();
	// 取りに行っている要求。画面には出さないので、変わったことを知らせる SvelteMap にしない。
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	#pending = new Map<number, { controller: AbortController; done: Promise<void> }>();
	/** 最後に要った順の区切りの番号 (古い順)。捨てるのは古いものから。 */
	#used: number[] = [];
	#fetch: ChunkFetch<T>;

	constructor(
		first: { snapshot: string; total: number; items: (T | null)[] },
		fetch: ChunkFetch<T>
	) {
		this.snapshot = first.snapshot;
		this.total = first.total;
		this.#fetch = fetch;
		this.#chunks.set(0, first.items);
		this.#used.push(0);
	}

	/** `index` 行目。まだ取っていなければ `undefined`、消えた・非公開になった行は `null`。 */
	get(index: number): T | null | undefined {
		return this.#chunks.get(Math.floor(index / ARCHIVE_CHUNK_SIZE))?.[index % ARCHIVE_CHUNK_SIZE];
	}

	/** `index` 行目を含む区切りが要る。無ければ取りに行く。 */
	ensure(index: number): void {
		void this.load(Math.floor(index / ARCHIVE_CHUNK_SIZE));
	}

	/** `index` 行目を含む区切りを取り終えるまで待つ。 */
	async ensureLoaded(index: number): Promise<void> {
		if (index < 0 || index >= this.total) return;
		await this.load(Math.floor(index / ARCHIVE_CHUNK_SIZE));
	}

	/** 取れなかった区切りを取り直す。 */
	retry(chunk: number): void {
		this.failed.delete(chunk);
		void this.load(chunk);
	}

	/**
	 * `index` 行目を含む、手元にある連続した行の範囲 (`[start, end)`)。
	 * 音声とビューアーの並びを、取ってある行から作るのに使う。
	 */
	loadedRange(index: number): [number, number] {
		let first = Math.floor(index / ARCHIVE_CHUNK_SIZE);
		let last = first;
		while (first > 0 && this.#chunks.has(first - 1)) first -= 1;
		while (this.#chunks.has(last + 1)) last += 1;
		return [first * ARCHIVE_CHUNK_SIZE, Math.min((last + 1) * ARCHIVE_CHUNK_SIZE, this.total)];
	}

	/** 見えている所から遠い区切りの要求を取り消す。弾くようにスクロールしたとき、通らなくなった要求が詰まらないように。 */
	cancelOutside(start: number, end: number): void {
		const first = Math.floor(start / ARCHIVE_CHUNK_SIZE) - 1;
		const last = Math.floor(end / ARCHIVE_CHUNK_SIZE) + 1;
		for (const [chunk, { controller }] of this.#pending) {
			if (chunk < first || chunk > last) {
				controller.abort();
				this.#pending.delete(chunk);
			}
		}
	}

	/** 取りに行っている要求を全部やめる。一覧を取り直すとき・画面を離れるときに呼ぶ。 */
	dispose(): void {
		for (const { controller } of this.#pending.values()) controller.abort();
		this.#pending.clear();
	}

	load(chunk: number): Promise<void> {
		this.#touch(chunk);
		if (this.#chunks.has(chunk) || this.failed.has(chunk) || this.expired) return Promise.resolve();
		const running = this.#pending.get(chunk);
		if (running) return running.done;
		const controller = new AbortController();
		const done = this.#run(chunk, controller);
		this.#pending.set(chunk, { controller, done });
		return done;
	}

	async #run(chunk: number, controller: AbortController): Promise<void> {
		try {
			const result = await this.#fetch(
				this.snapshot,
				chunk * ARCHIVE_CHUNK_SIZE,
				controller.signal
			);
			if (controller.signal.aborted) return;
			if ('expired' in result) {
				this.expired = true;
				return;
			}
			this.#chunks.set(chunk, result.items);
			if (result.stale) this.stale = true;
			this.#evict();
		} catch {
			if (!controller.signal.aborted) this.failed.add(chunk);
		} finally {
			if (this.#pending.get(chunk)?.controller === controller) this.#pending.delete(chunk);
		}
	}

	#touch(chunk: number) {
		const at = this.#used.indexOf(chunk);
		if (at >= 0) this.#used.splice(at, 1);
		this.#used.push(chunk);
	}

	#evict() {
		while (this.#chunks.size > MAX_CHUNKS) {
			const oldest = this.#used.find((chunk) => this.#chunks.has(chunk));
			if (oldest === undefined) return;
			this.#used.splice(this.#used.indexOf(oldest), 1);
			this.#chunks.delete(oldest);
		}
	}
}
