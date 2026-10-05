import { untrack } from 'svelte';
import type { ActionReturn } from 'svelte/action';
import type { Attachment } from 'svelte/attachments';
import {
	SHADOW_ITEM_MARKER_PROPERTY_NAME,
	dndzone,
	type DndEvent,
	type Options as DndZoneOptions
} from 'svelte-dnd-action';

/** 並べ替えで周りの行が動く時間。行の `animate:flip` にも同じ値を渡す。 */
export const SORTABLE_FLIP_DURATION_MS = 150;

export type SortableEntry<T> = {
	id: string | number;
	item: T;
	[SHADOW_ITEM_MARKER_PROPERTY_NAME]?: boolean;
};

type SortableZoneOptions<T> = {
	/** 今の並び。 */
	items: () => readonly T[];
	/** 行の識別子。省くと行のオブジェクトそのもので見分ける。 */
	key?: (item: T) => string | number;
	/** 並べ替えを止めているか。 */
	locked: () => boolean;
	/** 落としたときの並び。 */
	onReorder: (items: T[]) => void;
};

/**
 * 一覧の行を、行頭のつまみのドラッグで並べ替える (→ docs/ui.md「UI 全般」)。
 *
 * 一覧の要素に `zone`、つまみに `handle(entry.id)` を付け、`entries` を keyed each で並べる。
 * 行は一覧の要素の直接の子にし、`animate:flip` に `SORTABLE_FLIP_DURATION_MS` を渡す。
 *
 * ADR: svelte-dnd-action の `dragHandleZone` / `dragHandle` は使わない。ドラッグの可否を
 * モジュール全体で1つだけ持つので、同じページの一覧どうしで混ざるうえ、つまみに
 * `tabindex` と `role="button"` を付けてキーボードの止まり先を増やしてしまう。
 * 代わりに `dndzone` を、つまみを押している間だけドラッグを許す設定で使う。
 */
export class SortableZone<T extends object> {
	#options: SortableZoneOptions<T>;
	#ids = new WeakMap<T, number>();
	#nextId = 0;
	#type = `sortable-${Math.random().toString(36).slice(2)}`;
	#zone: ActionReturn<DndZoneOptions<SortableEntry<T>>> | null = null;
	#node: HTMLElement | null = null;
	#handlePressed = false;
	#considering = $state.raw<SortableEntry<T>[] | null>(null);

	/** つまみを押している行。押しただけで動かしていない間も、掴めたことを見せるために使う。 */
	pressedId = $state<string | number | null>(null);

	/** 描画する行。ドラッグ中は、落とす位置に置いた仮の行 (影) を含む。 */
	entries = $derived.by((): SortableEntry<T>[] => {
		if (this.#considering !== null) return this.#considering;
		return this.#options.items().map((item) => ({ id: this.#idOf(item), item }));
	});

	constructor(options: SortableZoneOptions<T>) {
		this.#options = options;

		// 並びと止める状態が変わったら、ゾーンへ渡し直す。
		$effect(() => {
			const config = this.#config();
			this.#zone?.update?.(config);
		});

		// つまみを離したら (ドラッグにならなかった場合も含めて)、ドラッグを止めた設定に戻す。
		$effect(() => {
			const release = () => this.#release();
			// FIX: svelte-dnd-action はドラッグの終わりを touchend でしか受けないので、OS がタッチを
			// 中断した (touchcancel) ときは touchend を投げて終わらせる。受け側はイベントの中身を見ない。
			// pointercancel では解かない。touchcancel より先に届き、ここで解くと touchend を投げられなくなる。
			const cancel = () => {
				if (this.#handlePressed) window.dispatchEvent(new Event('touchend'));
				this.#release();
			};
			window.addEventListener('mouseup', release);
			window.addEventListener('touchend', release);
			window.addEventListener('touchcancel', cancel);
			return () => {
				window.removeEventListener('mouseup', release);
				window.removeEventListener('touchend', release);
				window.removeEventListener('touchcancel', cancel);
			};
		});
	}

	zone: Attachment<HTMLElement> = (node) => {
		const consider = (event: Event) => {
			this.#considering = (event as CustomEvent<DndEvent<SortableEntry<T>>>).detail.items;
		};
		const finalize = (event: Event) => {
			const items = (event as CustomEvent<DndEvent<SortableEntry<T>>>).detail.items;
			this.#considering = null;
			this.#release();
			this.#options.onReorder(items.map((entry) => entry.item));
		};
		node.addEventListener('consider', consider);
		node.addEventListener('finalize', finalize);
		// 設定の変化は上の $effect で渡すので、ここでは追跡しない (追跡するとゾーンが作り直される)。
		const zone: ActionReturn<DndZoneOptions<SortableEntry<T>>> = dndzone(
			node,
			untrack(() => this.#config())
		);
		this.#zone = zone;
		this.#node = node;
		return () => {
			node.removeEventListener('consider', consider);
			node.removeEventListener('finalize', finalize);
			zone.destroy?.();
			if (this.#zone === zone) {
				this.#zone = null;
				this.#node = null;
			}
		};
	};

	handle(id: string | number): Attachment<HTMLElement> {
		return (node) => {
			const press = (event: PointerEvent) => {
				if (event.button !== 0 || untrack(() => this.#options.locked())) return;
				this.#handlePressed = true;
				// ドラッグは行の mousedown / touchstart で始まる。同じ操作が行へ伝わる前に、
				// ドラッグを許した設定をゾーンへ渡しておく。
				this.#zone?.update?.(untrack(() => this.#config()));
				this.pressedId = id;
				// 触覚フィードバック。Vibration API のある端末 (Android) だけで効く。
				if (event.pointerType === 'touch') navigator.vibrate?.(10);
			};
			node.addEventListener('pointerdown', press);
			return () => node.removeEventListener('pointerdown', press);
		};
	}

	/** 落とす位置に置いた仮の行か。 */
	isShadow(entry: SortableEntry<T>): boolean {
		return entry[SHADOW_ITEM_MARKER_PROPERTY_NAME] === true;
	}

	/** つまみを押している行か (仮の行は含めない)。 */
	isPressed(entry: SortableEntry<T>): boolean {
		return !this.isShadow(entry) && this.pressedId === entry.id;
	}

	#idOf(item: T): string | number {
		if (this.#options.key) return this.#options.key(item);
		let id = this.#ids.get(item);
		if (id === undefined) {
			id = this.#nextId++;
			this.#ids.set(item, id);
		}
		return id;
	}

	#config() {
		return {
			items: this.entries,
			type: this.#type,
			flipDurationMs: SORTABLE_FLIP_DURATION_MS,
			dragDisabled: !this.#handlePressed || this.#options.locked(),
			dropTargetStyle: {},
			transformDraggedElement: this.#fitDraggedRow,
			// 表として読み上げさせるため、role="list" などを付けさせない。
			// キーボードでは各行の上下ボタンで並べ替える。
			autoAriaDisabled: true,
			zoneTabIndex: -1,
			zoneItemTabIndex: -1
		};
	}

	/**
	 * 運ぶ行の複製が表の行なら、元の行と同じ列幅で描く。複製は表の外に fixed で置かれ、
	 * `tr` はブロックとして描かれてセルが中身の幅で並んでしまうため。
	 * `index` は落とす位置に置いた仮の行で、元の行と同じセルの幅を持つ。
	 */
	#fitDraggedRow = (element?: HTMLElement, _data?: unknown, index?: number) => {
		if (element?.tagName !== 'TR' || index === undefined) return;
		const source = this.#node?.children[index];
		if (!source) return;
		element.style.display = 'table';
		element.style.tableLayout = 'fixed';
		Array.from(element.children).forEach((cell, i) => {
			const width = source.children[i]?.getBoundingClientRect().width;
			if (width !== undefined) (cell as HTMLElement).style.width = `${width}px`;
		});
	};

	#release() {
		if (!this.#handlePressed) return;
		this.#handlePressed = false;
		this.pressedId = null;
		this.#zone?.update?.(untrack(() => this.#config()));
	}
}
