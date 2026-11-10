import { client } from '$lib/api/client';
import type { ErrorDialogState } from '$lib/error-dialog-state.svelte';

/**
 * 自分のリカバリコードを作る (→ docs/access.md「リカバリコード」)。作ったものは `code` に入り、
 * `RecoveryCodeDialog` に `bind:code` で渡すと開く。作り直しの確かめは呼び出し側が持つ。
 */
export class RecoveryCodeCreate {
	creating = $state(false);
	code = $state<string | null>(null);

	#errorDialog: ErrorDialogState;

	constructor(errorDialog: ErrorDialogState) {
		this.#errorDialog = errorDialog;
	}

	async create() {
		if (this.creating) return;
		this.creating = true;
		const result = await this.#errorDialog.attempt(() => client.POST('/api/v1/auth/recovery-code'));
		this.creating = false;
		if (result?.data) this.code = result.data.recoveryCode;
	}
}
