/**
 * 端末に覚えさせる設定の読み書き (`localStorage`)。
 * プライベートブラウズ等で使えなくても動くよう、読めなければ `null`、書けなければ何もしない。
 * 書けなくても、呼び出し側の `$state` で今の画面での切り替えは効く。
 */
export function readStored(key: string): string | null {
	try {
		return globalThis.localStorage?.getItem(key) ?? null;
	} catch {
		return null;
	}
}

export function writeStored(key: string, value: string): void {
	try {
		globalThis.localStorage?.setItem(key, value);
	} catch {
		// 覚えられないだけで、呼び出し側の状態は変わっている。
	}
}
