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
	/** 区切って取る一覧での位置。シャッフルを切ったとき、今の曲の次から一覧の並びで続けるのに使う。 */
	position?: number;
};

/**
 * 並びの続きを足す口 (→ docs/archive.md「続けて鳴らす音声とビューアー」)。区切って取るアーカイブの一覧は、
 * 手元にある曲だけで並びを作るので、終わりに近づいたら続きを取って足す。続きが無ければ空を返す。
 */
export type TrackSource = {
	more: () => Promise<Track[]>;
	/** 一覧の全部の音声を混ぜた順で返す口。手元に全部が無い一覧は、混ぜるのをサーバーに任せる。 */
	shuffled?: () => TrackSource;
	/** `track` の次から一覧の並びで返す口。シャッフルを切ったとき、今の曲が手元の並びに無ければ使う。 */
	after?: (track: Track) => TrackSource;
};

/**
 * 曲が終わったときの動き (→ docs/ui.md「音声のページ内プレイヤー」)。
 * `off` は1曲で止まる、`list` は最後の曲まで続けて止まる、`all` は最後の曲の次に最初へ戻る、`one` は同じ曲を繰り返す。
 */
export type RepeatMode = 'off' | 'list' | 'all' | 'one';

/** 切り替えボタンを押したときに進む順。 */
export const REPEAT_MODES: readonly RepeatMode[] = ['off', 'list', 'all', 'one'];

/**
 * 残り何曲で続きを取りに行くか。曲の差し替えは終わった操作の中で同期的に行う (下の ADR) ので、
 * 終わってから取りに行くと、ロック画面で続けて鳴らしているときに止まる。曲の長さより取る時間はずっと短い。
 */
const PREFETCH_REMAINING = 10;

/** 曲が終わったときの動きと、シャッフルを端末に覚えさせるキー。 */
const REPEAT_STORAGE_KEY = 'weblav:audio-repeat';
const SHUFFLE_STORAGE_KEY = 'weblav:audio-shuffle';
/** 前の「続けて再生」のオン・オフ。覚えていた端末では、オンなら `list` から始める。 */
const LEGACY_AUTO_ADVANCE_STORAGE_KEY = 'weblav:audio-auto-advance';

function storedRepeat(): RepeatMode {
	const stored = readStored(REPEAT_STORAGE_KEY);
	const mode = REPEAT_MODES.find((mode) => mode === stored);
	if (mode) return mode;
	return readStored(LEGACY_AUTO_ADVANCE_STORAGE_KEY) === 'true' ? 'list' : 'off';
}

/** 新しい配列に混ぜて返す (Fisher–Yates)。 */
function shuffledCopy<T>(items: readonly T[]): T[] {
	const copy = [...items];
	for (let i = copy.length - 1; i > 0; i--) {
		const j = Math.floor(Math.random() * (i + 1));
		[copy[i], copy[j]] = [copy[j], copy[i]];
	}
	return copy;
}

// ADR: 前後の曲は再生を始めた時点の一覧を写し取って決める。一覧の画面を離れたり
// 絞り込みを変えたりしても、鳴っている曲の前後が入れ替わらないようにするため。
let queue = $state<Track[]>([]);
let index = $state(-1);
/** 並びの続きを足す口。続きが無いと分かったら外す。外れると全曲リピートで端から戻れるようになるので、画面に知らせる。 */
let source = $state.raw<TrackSource | null>(null);
/**
 * 続きを取りに行っている口。シャッフルを切り替えて同じ口に戻ったとき、取っている最中の口へもう一度頼まないため
 * (口の中の位置は取り終えてから進むので、二重に頼むと同じ区切りが返り、続きが無いと見誤る)。
 */
let fetching: TrackSource | null = null;
let repeat = $state(storedRepeat());
let shuffle = $state(readStored(SHUFFLE_STORAGE_KEY) === 'true');
/** シャッフル中だけ、混ぜる前の並びと続きの口。切ったときに、今の曲の位置から元の並びに戻すため。 */
let ordered: { queue: Track[]; source: TrackSource | null } | null = null;
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

/** `queue` の後ろに、まだ無い曲を足した並び。 */
function appendNew(queue: Track[], tracks: Track[]): Track[] {
	const known = new Set(queue.map((track) => track.src));
	const added = tracks.filter((track) => !known.has(track.src));
	return added.length === 0 ? queue : [...queue, ...added];
}

/** 最後の曲の次に最初へ戻れるか。続きを取り切ってからでないと、まだ取っていない曲を飛ばすことになる。 */
function wraps() {
	return repeat === 'all' && source === null && queue.length > 0;
}

/** 同じ曲を繰り返すときは、要素に任せる。曲の終わりを待って鳴らし直すより途切れず、ロック画面でも止まらない。 */
function applyLoop() {
	if (element) element.loop = repeat === 'one';
}

/**
 * 今の曲を先頭にして、残りを混ぜた並びにする。混ぜる口があれば並びの全部を混ぜたものを続きとして取り、
 * 無ければ手元の並び (一覧の全部) を混ぜる。
 */
function enterShuffle() {
	const current = queue[index];
	if (!current) return;
	ordered = { queue, source };
	const server = source?.shuffled?.();
	queue = server ? [current] : [current, ...shuffledCopy(queue.filter((_, i) => i !== index))];
	index = 0;
	source = server ?? null;
}

/** 混ぜる前の並びに戻し、今の曲の次から続ける。 */
function leaveShuffle() {
	const current = queue[index];
	const saved = ordered;
	ordered = null;
	if (!current || !saved) return;
	const found = saved.queue.findIndex((track) => track.src === current.src);
	if (found >= 0) {
		queue = saved.queue;
		index = found;
		source = saved.source;
	} else {
		queue = [current];
		index = 0;
		source = saved.source?.after?.(current) ?? null;
	}
}

/** 終わりに近づいていたら、並びの続きを取って足しておく。 */
function prefetch() {
	const current = source;
	if (!current || fetching === current || queue.length - 1 - index > PREFETCH_REMAINING) return;
	fetching = current;
	void current
		.more()
		.then((tracks) => {
			// 取っている間にシャッフルをオンにしたら、混ぜる前の並びに足しておく。口の中の位置はもう進んでいるので、
			// 捨てるとオフにしたときにその区切りを飛ばす。
			if (ordered?.source === current) {
				ordered.queue = appendNew(ordered.queue, tracks);
				return;
			}
			if (source !== current) return;
			const added = appendNew(queue, tracks);
			if (added.length > queue.length) queue = added;
			else if (ordered && queue.length === 1) {
				// 混ぜた結果を作れなかった (元の結果が捨てられていた) ので、手元の並びを混ぜる。
				queue = [queue[0], ...shuffledCopy(ordered.queue.filter((t) => t.src !== queue[0].src))];
				source = null;
			} else source = null;
		})
		.catch(() => {
			// 取れなければ、今ある並びの終わりで止まる。次に曲を替えたときにまた試す。
		})
		.finally(() => {
			if (fetching === current) fetching = null;
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
		return index > 0 || (index === 0 && wraps() && queue.length > 1);
	},
	get hasNext() {
		return index >= 0 && (index < queue.length - 1 || wraps());
	},
	get paused() {
		return paused;
	},
	get failed() {
		return failed;
	},
	get repeat() {
		return repeat;
	},
	set repeat(value: RepeatMode) {
		repeat = value;
		writeStored(REPEAT_STORAGE_KEY, value);
		applyLoop();
	},
	get shuffle() {
		return shuffle;
	},
	/** 鳴っている曲はそのまま続け、その後の並びだけを替える。 */
	set shuffle(value: boolean) {
		if (value === shuffle) return;
		shuffle = value;
		writeStored(SHUFFLE_STORAGE_KEY, String(value));
		if (index < 0) return;
		if (value) enterShuffle();
		else leaveShuffle();
		prefetch();
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
		applyLoop();
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
		ordered = null;
		if (shuffle) enterShuffle();
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
		if (index > 0) index -= 1;
		else if (index === 0 && wraps() && queue.length > 1) index = queue.length - 1;
		else return;
		load();
	},
	/** `play` なら、止めていても次の曲を鳴らす (曲が終わって次へ進むとき)。 */
	next({ play = false }: { play?: boolean } = {}) {
		if (index < 0) return;
		if (index < queue.length - 1) index += 1;
		else if (wraps()) {
			// シャッフルなら、1周ごとに混ぜ直す (同じ順を繰り返さない)。終わった曲が続けて鳴らないよう、先頭には置かない。
			if (shuffle) {
				const finished = queue[index];
				const rest = shuffledCopy(queue.filter((_, i) => i !== index));
				queue =
					rest.length === 0 ? [finished] : [rest[0], ...shuffledCopy([finished, ...rest.slice(1)])];
			}
			index = 0;
		} else return;
		if (play) paused = false;
		load();
	},
	/** 要素の ended イベントで、曲が終わったときの動き (`repeat`) に従う。`one` は要素が繰り返すので届かない。 */
	ended() {
		// 曲の終わりで一時停止の状態になっているので、次の曲は明示的に鳴らす。
		if (repeat === 'list' || repeat === 'all') nowPlaying.next({ play: true });
	},
	stop() {
		queue = [];
		index = -1;
		source = null;
		ordered = null;
		load();
		// 閉じた後も音の処理を動かし続けないよう止める。次の再生の操作の中で再開する。
		// 一時停止では止めない。ロック画面からの再生は操作の中と見なされず、再開できないことがあるため。
		void graph?.context.suspend();
	}
};
