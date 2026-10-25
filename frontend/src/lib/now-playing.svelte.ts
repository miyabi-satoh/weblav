import { readStored, writeStored } from '$lib/stored';

/**
 * ページ下部の音声プレイヤーが再生する対象。同時に鳴らせるのは1トラックのみ
 * (→ docs/ui.md「音声のページ内プレイヤー」)なので、ページ・コンポーネント単位ではなくモジュール
 * スコープの単一の$stateで持つ。
 */
export type Track = {
	src: string;
	title: string;
	/** URL のファイルの元の URL。再生できなかったとき、案内からここを開く (→ docs/ui.md「URL のファイル」)。 */
	originalUrl?: string;
};

/**
 * 並びの続きを足す口 (→ docs/archive.md「続けて鳴らす音声とビューアー」)。区切って取るアーカイブの一覧は、
 * 手元にある曲だけで並びを作るので、終わりに近づいたら続きを取って足す。続きが無ければ空を返す。
 */
export type TrackSource = { more: () => Promise<Track[]> };

/**
 * 残り何曲で続きを取りに行くか。曲の差し替えは終わった操作の中で同期的に行う (下の ADR) ので、
 * 終わってから取りに行くと、ロック画面で続けて鳴らしているときに止まる。曲の長さより取る時間はずっと短い。
 */
const PREFETCH_REMAINING = 10;

/** 「続けて再生」の状態を端末に覚えさせるキー。 */
const AUTO_ADVANCE_STORAGE_KEY = 'weblav:audio-auto-advance';

// ADR: 前後の曲は再生を始めた時点の一覧を写し取って決める。一覧の画面を離れたり
// 絞り込みを変えたりしても、鳴っている曲の前後が入れ替わらないようにするため。
let queue = $state<Track[]>([]);
let index = $state(-1);
/** 並びの続きを足す口。続きが無いと分かったら外す。 */
let source: TrackSource | null = null;
let fetchingMore = false;
let autoAdvance = $state(readStored(AUTO_ADVANCE_STORAGE_KEY) === 'true');
// 前・次の曲では止めたまま曲だけ替えるので、一時停止の状態は曲をまたいで持つ。
let paused = $state(true);
/** 今の曲を読み込めなかった。曲を替えるまで、プレイヤーは再生位置の代わりに案内を出す。 */
let failed = $state(false);

// ADR: `<audio>` は1つだけ置いて使い回し、曲の差し替えと play() は押された操作の中で同期的に行う。
// Safari は、操作の後に作り直した要素や、非同期に呼んだ play() を利用者の操作と見なさず再生を拒むため。
// 同じ要素なら、iOS でも曲の終わりから次の曲を鳴らせる。
let element: HTMLAudioElement | null = null;
/** 最後に呼んだ play() の番号。同じ要素を使い回すので、前の曲の play() の失敗が後から届くことがある。 */
let playRequest = 0;
let muted = $state(false);

/** スペクトラムアナライザーのための音の流れ (要素 → analyser → gain → 出力)。 */
type Graph = {
	element: HTMLAudioElement;
	context: AudioContext;
	analyser: AnalyserNode;
	gain: GainNode;
};
let graph = $state.raw<Graph | null>(null);

/**
 * 音を Web Audio に通す (→ docs/ui.md「音声のページ内プレイヤー」)。
 * AudioContext の作成と再開は、操作の中に収めるため、`play()` より前に同期で呼ぶ。
 */
function connectGraph(target: HTMLAudioElement) {
	if (graph?.element !== target) {
		void graph?.context.close();
		graph = null;
		let context: AudioContext | undefined;
		try {
			// FIX: WebKit は Web Audio の音を端末の消音に従わせる (`<audio>` だけなら従わない)。
			// 今までどおり消音モードでも鳴るよう、使えるときは再生用の音声セッションにする。
			const session = (navigator as Navigator & { audioSession?: { type: string } }).audioSession;
			if (session) session.type = 'playback';
			context = new AudioContext();
			const analyser = context.createAnalyser();
			const gain = context.createGain();
			gain.gain.value = muted ? 0 : 1;
			context
				.createMediaElementSource(target)
				.connect(analyser)
				.connect(gain)
				.connect(context.destination);
			target.muted = false;
			graph = { element: target, context, analyser, gain };
		} catch {
			// Web Audio を使えないブラウザーでは、棒を出さず、ミュートは要素で行う。
			void context?.close();
			target.muted = muted;
			return;
		}
	}
	if (graph && graph.context.state !== 'running') void graph.context.resume();
}

function resume() {
	const target = element;
	if (!target) return;
	connectGraph(target);
	paused = false;
	const request = ++playRequest;
	target.play().catch((error: unknown) => {
		if (target !== element || request !== playRequest) return;
		// 曲を替えて中断された (AbortError) ときは、替えた先の再生が状態を決める。
		if (error instanceof DOMException && error.name === 'AbortError') {
			paused = target.paused;
			return;
		}
		// FIX: WebKit は読み込みに失敗して拒んでも `paused` を false のまま残し、pause も送らない。
		paused = true;
	});
}

/** 終わりに近づいていたら、並びの続きを取って足しておく。 */
function prefetch() {
	const current = source;
	if (!current || fetchingMore || queue.length - 1 - index > PREFETCH_REMAINING) return;
	fetchingMore = true;
	void current
		.more()
		.then((tracks) => {
			if (source !== current) return;
			const known = new Set(queue.map((track) => track.src));
			const added = tracks.filter((track) => !known.has(track.src));
			if (added.length === 0) source = null;
			else queue = [...queue, ...added];
		})
		.catch(() => {
			// 取れなければ、今ある並びの終わりで止まる。次に曲を替えたときにまた試す。
		})
		.finally(() => {
			if (source === current) fetchingMore = false;
		});
}

/** 今の曲を要素に読み込ませる。止めていなければ鳴らす。 */
function load() {
	prefetch();
	if (!element) return;
	failed = false;
	const track = queue[index];
	if (!track) {
		element.pause();
		element.removeAttribute('src');
		element.load();
		return;
	}
	element.src = track.src;
	if (!paused) resume();
}

export const nowPlaying = {
	get current(): Track | null {
		return queue[index] ?? null;
	},
	get hasPrevious() {
		return index > 0;
	},
	get hasNext() {
		return index >= 0 && index < queue.length - 1;
	},
	get paused() {
		return paused;
	},
	get failed() {
		return failed;
	},
	get autoAdvance() {
		return autoAdvance;
	},
	set autoAdvance(value: boolean) {
		autoAdvance = value;
		writeStored(AUTO_ADVANCE_STORAGE_KEY, String(value));
	},
	get muted() {
		return muted;
	},
	set muted(value: boolean) {
		muted = value;
		if (graph) graph.gain.gain.value = value ? 0 : 1;
		else if (element) element.muted = value;
	},
	/** 鳴っている音の周波数を測るもの。Web Audio に通す前 (最初の再生の前) と、使えないブラウザーでは null。 */
	get analyser(): AnalyserNode | null {
		return graph?.analyser ?? null;
	},
	/** プレイヤーの `<audio>` を登録する。戻り値で登録を外す。 */
	attach(audio: HTMLAudioElement) {
		element = audio;
		return () => {
			if (element === audio) element = null;
		};
	},
	/** 要素の play / pause イベントで、一時停止の状態を実際に合わせる。 */
	syncPaused() {
		if (element) paused = element.paused;
	},
	/** 要素の error イベントで、今の曲を読み込めなかったことにする。 */
	markFailed() {
		// 曲を外したとき (`load()` で src を外す) に届くものは、曲の失敗ではない。
		if (!queue[index]) return;
		failed = true;
		paused = true;
	},
	/**
	 * 一覧の行から再生を始める。止めていても鳴らす。
	 * `list` は、押した行が並んでいた一覧の音声 (表示順)。1つの一覧の中で `src` は重ならない。
	 * `more` は、`list` の続きを足す口 (区切って取る一覧で渡す)。
	 */
	play(track: Track, list: Track[] = [track], more?: TrackSource) {
		// 今の曲の行なら、頭に戻さず続きから鳴らす (src を入れ直すと読み込み直しになる)。
		// 読み込めなかった曲は、押し直したら読み込み直す。
		if (element && queue[index]?.src === track.src && !failed) {
			resume();
			return;
		}
		const found = list.findIndex((t) => t.src === track.src);
		queue = found < 0 ? [track] : list.map((t) => ({ ...t }));
		index = Math.max(found, 0);
		source = found < 0 ? null : (more ?? null);
		fetchingMore = false;
		paused = false;
		load();
	},
	togglePlay() {
		if (!element) return;
		// 読み込めなかった曲は、失敗した要素を鳴らし直しても鳴らないので、読み込み直す。
		if (failed) {
			paused = false;
			load();
		} else if (element.paused) resume();
		else element.pause();
	},
	pause() {
		element?.pause();
	},
	resume,
	previous() {
		if (index <= 0) return;
		index -= 1;
		load();
	},
	/** `play` なら、止めていても次の曲を鳴らす (続けて再生)。 */
	next({ play = false }: { play?: boolean } = {}) {
		if (index < 0 || index >= queue.length - 1) return;
		index += 1;
		if (play) paused = false;
		load();
	},
	stop() {
		queue = [];
		index = -1;
		source = null;
		load();
		// 閉じた後も音の処理を動かし続けないよう止める。次の再生の操作の中で再開する。
		// 一時停止では止めない。ロック画面からの再生は操作の中と見なされず、再開できないことがあるため。
		void graph?.context.suspend();
	}
};
