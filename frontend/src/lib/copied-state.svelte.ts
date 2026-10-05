/** コピーの成功を、ボタンのアイコンで見せておく時間 (ミリ秒)。読み取れて、次の操作の邪魔にならない長さ。 */
const COPIED_DURATION_MS = 2000;

/**
 * コピーに成功したあと、しばらくボタンのアイコンをチェックマークに変えるための状態
 * (→ トーストは大げさ)。続けてコピーしたら、そこから数え直す。
 */
export class CopiedState {
	copied = $state(false);
	#timeout: ReturnType<typeof setTimeout> | undefined;

	/** コピーに成功したときに呼ぶ。 */
	show(): void {
		this.copied = true;
		clearTimeout(this.#timeout);
		this.#timeout = setTimeout(() => (this.copied = false), COPIED_DURATION_MS);
	}
}
