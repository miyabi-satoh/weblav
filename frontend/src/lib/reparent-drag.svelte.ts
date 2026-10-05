import { untrack } from 'svelte';
import type { Attachment } from 'svelte/attachments';

type ReparentDragOptions<T> = {
	/** ドラッグを止めているか (保存中など)。 */
	locked: () => boolean;
	/**
	 * `target` へ `dragged` を落としてよいか (循環参照・自分自身の除外は呼び出し側の役目)。
	 * `target` が `null` のときはルート直下 (`root()` を付けた要素) へ落とす。
	 */
	isDropTarget: (dragged: T, target: T | null) => boolean;
	/** 有効なドロップ先へ落としたときの処理。`target` が `null` ならルート直下。 */
	onDrop: (dragged: T, target: T | null) => void;
};

// ドロップ先が無いことを、ルート直下 (`null`) と区別するための印。
const NO_TARGET = Symbol('no-target');

/**
 * ドラッグ中にポインターが画面の上下端からこの距離 (px) に入ると、一覧を自動でスクロールする。
 * つまみは `touch-none` で指のスクロールを止めているため、画面外の行へ運ぶ手段がこれしかない。
 * 行の高さ (44px) より少し広く取り、端の1行の上で指を止めても掛かるようにする。
 */
const AUTO_SCROLL_EDGE = 48;
/** 端に入り切ったときの1フレームあたりのスクロール量 (px)。端に近いほど速くする。 */
const AUTO_SCROLL_MAX_STEP = 16;

/**
 * 一覧の行を掴んで、別の行(グループ)かルート直下の要素へドロップして親を付け替える (→ docs/ui.md「UI 全般」)。
 *
 * ADR: 並べ替えに使っている svelte-dnd-action (`sortable-zone.svelte.ts`) はドロップ先を
 * 「どのゾーンの何番目か」でしか表せず、行の上に落とす操作を持たないため、Pointer Events を直接使う。
 */
export class ReparentDrag<T extends { id: number }> {
	#options: ReparentDragOptions<T>;
	// 当たり判定で node → item を引く帳簿。テンプレートには出さないので素の Map でよい。
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- 上記理由により非リアクティブが意図
	#nodeToItem = new Map<Element, T>();
	#rootNode: HTMLElement | null = null;
	#pointerId: number | null = null;
	#handleNode: HTMLElement | null = null;
	#dragged: T | null = null;
	// 今のポインター位置にある有効なドロップ先。
	#target: T | null | typeof NO_TARGET = NO_TARGET;
	#x = 0;
	#y = 0;
	// 掴んだ点の、行の左上からのずれ。指に付けて運ぶ行の複製を、掴んだ点がずれないように置くため。
	#grabX = 0;
	#grabY = 0;
	#autoScrollFrame: number | null = null;

	/** 掴んでいる行の id。 */
	draggingId = $state<number | null>(null);
	/** 今ポインターの下にある、有効なドロップ先の行の id。 */
	dropTargetId = $state<number | null>(null);
	/** 今ポインターの下にある有効なドロップ先が、ルート直下の要素か。 */
	rootTargeted = $state(false);
	/**
	 * 指(ポインター)に付けて運ぶ行の複製を置く位置と大きさ (画面の座標)。掴んでいない間は `null`。
	 * 複製は呼び出し側が `pointer-events: none` で描く (当たり判定は複製の下の行で取るため)。
	 */
	ghost = $state<{ x: number; y: number; width: number; height: number } | null>(null);

	constructor(options: ReparentDragOptions<T>) {
		this.#options = options;
	}

	/** 一覧の各行の要素に付ける。ドロップ先の当たり判定にどの行も使うため、全行に付ける。 */
	row(item: T): Attachment<HTMLElement> {
		return (node) => {
			this.#nodeToItem.set(node, item);
			return () => {
				this.#nodeToItem.delete(node);
			};
		};
	}

	/** ルート直下へ移すドロップ先の要素に付ける。 */
	root(): Attachment<HTMLElement> {
		return (node) => {
			this.#rootNode = node;
			return () => {
				if (this.#rootNode === node) this.#rootNode = null;
			};
		};
	}

	/** 行のつまみに付ける。 */
	handle(item: T): Attachment<HTMLElement> {
		return (node) => {
			const press = (event: PointerEvent) => {
				// 既にドラッグ中(iPadで支えの指が別の行のつまみに触れた等)は無視する。
				// 上書きすると、先に掴んでいた行の pointerup が id 不一致で無視され、
				// setPointerCapture も解放されないままになる。
				if (this.#pointerId !== null) return;
				if (event.button !== 0 || untrack(() => this.#options.locked())) return;
				// リンクの選択を起こさない。iOS の長押しは下の touchstart で止める。
				event.preventDefault();
				this.#pointerId = event.pointerId;
				this.#handleNode = node;
				this.#dragged = item;
				this.draggingId = item.id;
				this.#x = event.clientX;
				this.#y = event.clientY;
				this.#startGhost(node);
				node.setPointerCapture(event.pointerId);
				window.addEventListener('pointermove', this.#move);
				window.addEventListener('pointerup', this.#release);
				window.addEventListener('pointercancel', this.#release);
				// マウスのホイールでスクロールすると pointermove が来ないまま下の行が入れ替わるので、
				// スクロールでも当たり判定をやり直す。
				window.addEventListener('scroll', this.#retarget, { passive: true });
			};
			// FIX: iOS の WebKit は pointerdown の preventDefault では長押しの文字選択とメニューを止めない。
			// touchstart を passive でない形で止める。
			const holdTouch = (event: TouchEvent) => {
				if (!untrack(() => this.#options.locked())) event.preventDefault();
			};
			node.addEventListener('pointerdown', press);
			node.addEventListener('touchstart', holdTouch, { passive: false });
			return () => {
				node.removeEventListener('pointerdown', press);
				node.removeEventListener('touchstart', holdTouch);
				// ドラッグ中に画面ごと破棄されたとき (ページ遷移など)、window のリスナーと
				// 自動スクロールを残さない。
				if (this.#handleNode === node) this.#end();
			};
		};
	}

	#move = (event: PointerEvent) => {
		if (event.pointerId !== this.#pointerId) return;
		this.#x = event.clientX;
		this.#y = event.clientY;
		if (this.ghost !== null) {
			this.ghost = { ...this.ghost, x: this.#x - this.#grabX, y: this.#y - this.#grabY };
		}
		this.#retarget();
		if (this.#autoScrollStep() !== 0 && this.#autoScrollFrame === null) {
			this.#autoScrollFrame = requestAnimationFrame(this.#autoScroll);
		}
	};

	// つまみから行の要素を辿り、掴んだ時点の行の位置に複製を置く。
	#startGhost(handle: HTMLElement) {
		let row = handle.parentElement;
		while (row !== null && !this.#nodeToItem.has(row)) row = row.parentElement;
		if (row === null) return;
		const rect = row.getBoundingClientRect();
		this.#grabX = this.#x - rect.left;
		this.#grabY = this.#y - rect.top;
		this.ghost = { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
	}

	#retarget = () => {
		const dragged = this.#dragged;
		if (dragged === null) return;
		const target = this.#targetAt(this.#x, this.#y);
		const valid =
			target !== NO_TARGET &&
			target?.id !== dragged.id &&
			this.#options.isDropTarget(dragged, target);
		this.#target = valid ? target : NO_TARGET;
		this.dropTargetId = valid ? (target?.id ?? null) : null;
		this.rootTargeted = valid && target === null;
	};

	// 上端は、ルートへの落とし先が画面に出ていればその下から数える (その上ではルートへ落とすため)。
	#autoScrollStep(): number {
		const top = Math.max(0, this.#rootNode?.getBoundingClientRect().bottom ?? 0);
		const intoTop = top + AUTO_SCROLL_EDGE - this.#y;
		if (this.#y >= top && intoTop > 0) {
			return -Math.ceil((AUTO_SCROLL_MAX_STEP * intoTop) / AUTO_SCROLL_EDGE);
		}
		const intoBottom = this.#y - (window.innerHeight - AUTO_SCROLL_EDGE);
		if (intoBottom > 0) {
			return Math.ceil(
				(AUTO_SCROLL_MAX_STEP * Math.min(intoBottom, AUTO_SCROLL_EDGE)) / AUTO_SCROLL_EDGE
			);
		}
		return 0;
	}

	#autoScroll = () => {
		this.#autoScrollFrame = null;
		if (this.#dragged === null) return;
		const step = this.#autoScrollStep();
		if (step === 0) return;
		window.scrollBy(0, step);
		this.#autoScrollFrame = requestAnimationFrame(this.#autoScroll);
	};

	// pointerup・pointercancel共通。cancel時はドロップ先を見ずに終える。
	#release = (event: PointerEvent) => {
		if (event.pointerId !== this.#pointerId) return;
		const dropped = event.type === 'pointerup';
		if (dropped) {
			// 離した位置で当て直す (最後の pointermove から後に下の行が変わっていることがある)。
			this.#x = event.clientX;
			this.#y = event.clientY;
			this.#retarget();
		}
		const dragged = this.#dragged;
		const target = this.#target;
		this.#end();
		if (dropped && dragged !== null && target !== NO_TARGET) this.#options.onDrop(dragged, target);
	};

	// ドラッグの状態・リスナー・自動スクロールを片付ける。
	#end() {
		window.removeEventListener('pointermove', this.#move);
		window.removeEventListener('pointerup', this.#release);
		window.removeEventListener('pointercancel', this.#release);
		window.removeEventListener('scroll', this.#retarget);
		if (this.#autoScrollFrame !== null) cancelAnimationFrame(this.#autoScrollFrame);
		this.#autoScrollFrame = null;
		this.#dragged = null;
		this.#target = NO_TARGET;
		this.#pointerId = null;
		this.#handleNode = null;
		this.draggingId = null;
		this.dropTargetId = null;
		this.rootTargeted = false;
		this.ghost = null;
	}

	// pointermove のたびに全行の getBoundingClientRect() を呼ぶとレイアウト計算が
	// 件数分走るため、elementFromPoint() で1点だけ当てて祖先を辿る形にする。
	// ルート直下の要素なら `null`、何も無ければ NO_TARGET。
	#targetAt(x: number, y: number): T | null | typeof NO_TARGET {
		for (let node = document.elementFromPoint(x, y); node; node = node.parentElement) {
			if (node === this.#rootNode) return null;
			const item = this.#nodeToItem.get(node);
			if (item) return item;
		}
		return NO_TARGET;
	}
}
