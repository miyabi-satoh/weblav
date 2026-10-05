/**
 * フォームが「開いた時点から変わっているか」の判定。
 *
 * 入力途中のダイアログを外側クリックや Esc で閉じてしまわないためのガードに使う
 * (`Dialog.Content` の `interactOutsideBehavior` / `escapeKeydownBehavior`)。
 *
 * ADR: 値ごとの比較ではなく、開いた時点のスナップショットとの構造比較にする。
 * 項目が増えたときに比較の更新を忘れても、スナップショットを作る関数が1つなので
 * 判定が静かにずれない。項目はいずれもプリミティブなので `JSON.stringify` で足りる。
 */
export class FormDirtyState<T> {
	#read: () => T;
	#pristine = $state.raw<T | null>(null);

	/** @param read フォームの今の値を、比較できるプリミティブの集まりとして返す。 */
	constructor(read: () => T) {
		this.#read = read;
	}

	/** 開いた (または保存した) 時点の値を、これ以降の比較の基準にする。 */
	markPristine(): void {
		this.#pristine = this.#read();
	}

	get dirty(): boolean {
		return (
			this.#pristine !== null && JSON.stringify(this.#read()) !== JSON.stringify(this.#pristine)
		);
	}
}
