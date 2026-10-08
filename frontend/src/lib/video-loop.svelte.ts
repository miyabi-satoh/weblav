import { readStored, writeStored } from '$lib/stored';

/** 動画の「繰り返し再生」の状態を端末に覚えさせるキー。 */
const STORAGE_KEY = 'weblav:video-loop';

// 動画を替えても同じ状態で開くよう、部品ではなくモジュールで持つ。
let loop = $state(readStored(STORAGE_KEY) === 'true');

export const videoLoop = {
	get value() {
		return loop;
	},
	set value(value: boolean) {
		loop = value;
		writeStored(STORAGE_KEY, String(value));
	}
};
