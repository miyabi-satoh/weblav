/**
 * 動画のビューアーで、動画サイト (YouTube) と同じキーに割り当てる操作 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
 * 画面にボタンのある操作だけを割り当て、キーでしかできない操作は作らない。
 */
export type VideoKeyAction =
	| { type: 'toggle-play' }
	| { type: 'rewind' }
	| { type: 'forward' }
	| { type: 'toggle-mute' }
	| { type: 'toggle-fullscreen' }
	/** 全体のうちの割合 (0〜1) の位置へ移る。 */
	| { type: 'seek-fraction'; fraction: number };

// 要素のクラスでなく tagName で見る。単体テストを DOM の無い環境で流すため。
type KeyInput = Pick<
	KeyboardEvent,
	'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey' | 'isComposing'
> & { target: EventTarget | TargetLike | null };

type TargetLike = { tagName?: string; type?: string; isContentEditable?: boolean };

/** 打ち込む欄。文字のキーをその欄に渡す。シークバー (`range`) は打ち込まないので除く。 */
function isTypingTarget(target: TargetLike | null): boolean {
	if (target?.isContentEditable || target?.tagName === 'TEXTAREA') return true;
	return target?.tagName === 'INPUT' && target.type !== 'range';
}

/**
 * 押されたキーの操作。割り当てが無ければ `null`。左右キーは、動画の外ではビューアーの前後の移動に使うので、ここでは扱わない。
 */
export function videoKeyAction(event: KeyInput): VideoKeyAction | null {
	// ブラウザーや OS のショートカット (Ctrl+F など) と、YouTube でも別の操作の Shift 付きは横取りしない。
	if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || event.isComposing) {
		return null;
	}
	const target = event.target as TargetLike | null;
	if (isTypingTarget(target)) return null;
	const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
	switch (key) {
		case ' ':
			// ボタンの上のスペースは、そのボタンを押す操作に任せる。両方が効くと二重に切り替わるため。
			// リンクはスペースでは開かないので任せない (開いた直後のフォーカスは「新しいタブで開く」にある)。
			if (target?.tagName === 'BUTTON') return null;
			return { type: 'toggle-play' };
		case 'k':
			return { type: 'toggle-play' };
		case 'j':
			return { type: 'rewind' };
		case 'l':
			return { type: 'forward' };
		case 'm':
			return { type: 'toggle-mute' };
		case 'f':
			return { type: 'toggle-fullscreen' };
		case 'Home':
			return { type: 'seek-fraction', fraction: 0 };
		case 'End':
			return { type: 'seek-fraction', fraction: 1 };
	}
	if (/^[0-9]$/.test(key)) return { type: 'seek-fraction', fraction: Number(key) / 10 };
	return null;
}
