/** 動画の「繰り返し再生」の状態を端末に覚えさせるキー。 */
const STORAGE_KEY = 'weblav:video-loop';

// 動画を替えても同じ状態で開くよう、部品ではなくモジュールで持つ。
let loop = $state(read());

function read(): boolean {
	// プライベートブラウズ等で localStorage が使えなくても、オフとして動かす。
	try {
		return globalThis.localStorage?.getItem(STORAGE_KEY) === 'true';
	} catch {
		return false;
	}
}

export const videoLoop = {
	get value() {
		return loop;
	},
	set value(value: boolean) {
		loop = value;
		try {
			globalThis.localStorage?.setItem(STORAGE_KEY, String(value));
		} catch {
			// 覚えられなくても、今の画面での切り替えは効かせる。
		}
	}
};
