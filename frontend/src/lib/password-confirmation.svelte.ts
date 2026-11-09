/**
 * パスワードの確かめ直し (→ docs/access.md「操作の前のパスワードの確認」)。
 *
 * サーバーが `password_confirmation_required` を返したら、`client` の middleware が
 * `request()` を呼んでダイアログを開き、確かめられたら同じ要求を送り直す。
 * 操作ごとの画面は、確かめ直しのことを知らずに済む。
 */
class PasswordConfirmation {
	open = $state(false);
	#pending: Promise<boolean> | null = null;
	#resolve: ((confirmed: boolean) => void) | null = null;

	/**
	 * ダイアログを開き、確かめられたら `true`、やめたら `false` を返す。
	 * 開いている間に別の要求も断られたら、同じダイアログの結果を待たせる (2枚重ねない)。
	 */
	request(): Promise<boolean> {
		if (this.#pending) return this.#pending;
		this.#pending = new Promise<boolean>((resolve) => {
			this.#resolve = resolve;
		});
		this.open = true;
		return this.#pending;
	}

	/** ダイアログから呼ぶ。確かめられたか、やめたか。 */
	settle(confirmed: boolean) {
		this.open = false;
		this.#resolve?.(confirmed);
		this.#resolve = null;
		this.#pending = null;
	}
}

export const passwordConfirmation = new PasswordConfirmation();
