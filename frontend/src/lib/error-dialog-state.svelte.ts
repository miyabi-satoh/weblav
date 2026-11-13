import type { ApiResult } from '#lib/api/client.ts';
import { GENERIC_ERROR_MESSAGE, errorMessage, isConfirmationCancelled } from '#lib/api/errors.ts';

/**
 * `ErrorDialog` の開閉と文言。
 *
 * ```svelte
 * <ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
 * ```
 */
export class ErrorDialogState {
	open = $state(false);
	message = $state('');
	/** 省略すると `ErrorDialog` の汎用タイトルになる。画面固有の文言があるときだけ渡す。 */
	title = $state<string | undefined>(undefined);

	show(message: string, title?: string) {
		this.message = message;
		this.title = title;
		this.open = true;
	}

	/** エラー応答の理由を出す。パスワードの確かめ直しをやめたものは出さない (`isConfirmationCancelled`)。 */
	showError(error: unknown, title?: string) {
		if (isConfirmationCancelled(error)) return;
		this.show(errorMessage(error), title);
	}

	/**
	 * 要求を送る。エラー応答と例外は理由をダイアログに出して `null` を返し、
	 * 成功したら応答をそのまま返す。
	 * `onSuccess` は成功したときの後処理 (画面の更新など)。ここで投げた例外も、
	 * 要求の失敗と同じく汎用のエラーとして出す。
	 */
	async attempt<R extends ApiResult>(
		request: () => Promise<R>,
		onSuccess?: (result: R) => void | Promise<void>
	): Promise<R | null> {
		try {
			const result = await request();
			if (!result.response.ok) {
				this.showError(result.error);
				return null;
			}
			await onSuccess?.(result);
			return result;
		} catch {
			this.show(GENERIC_ERROR_MESSAGE());
			return null;
		}
	}
}
