import type { Attachment } from 'svelte/attachments';

/** つまみの幅 (px)。`layout.css` の `media-seek` と揃える。range はつまみの中心を両端から半分ずつ内側で動かす。 */
const THUMB_WIDTH = 20;

/**
 * シークバー (`<input type="range">`) の、触れた位置へ動かしてそのままドラッグできるようにする。
 *
 * FIX: iOS の Safari の range は、つまみに触れたところからしかドラッグできず、溝を押しても動かない。
 * 再生中はつまみが進み続けるので、指を置いた所がつまみの手前になると、戻す方向へ動かしても掴めない。
 * 触れた位置から値を計算して `onseek` に渡し、どこに触れても掴めるようにする。キーボードの操作は range に任せる。
 */
export function seekDrag(onseek: (seconds: number) => void): Attachment<HTMLInputElement> {
	return (input) => {
		function valueAt(clientX: number): number {
			const rect = input.getBoundingClientRect();
			const ratio = (clientX - rect.left - THUMB_WIDTH / 2) / (rect.width - THUMB_WIDTH);
			return Math.min(Math.max(ratio, 0), 1) * (Number(input.max) || 0);
		}

		function handleDown(event: PointerEvent) {
			if (event.button !== 0) return;
			// range 自身のドラッグと二重に動かさない。止めるとフォーカスも移らないので、自分で渡す。
			// 渡さないと、続く左右キーがシークバーでなくビューアの前後の移動に回る。
			event.preventDefault();
			input.focus({ preventScroll: true });
			input.setPointerCapture(event.pointerId);
			onseek(valueAt(event.clientX));
		}

		function handleMove(event: PointerEvent) {
			if (input.hasPointerCapture(event.pointerId)) onseek(valueAt(event.clientX));
		}

		input.addEventListener('pointerdown', handleDown);
		input.addEventListener('pointermove', handleMove);
		return () => {
			input.removeEventListener('pointerdown', handleDown);
			input.removeEventListener('pointermove', handleMove);
		};
	};
}
