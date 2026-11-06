import { refreshAll } from '$app/navigation';
import { toast } from 'svelte-sonner';
import type { ErrorDialogState } from '$lib/error-dialog-state.svelte';
import type { FormDirtyState } from '$lib/form-dirty.svelte';
import { LeaveGuard } from '$lib/leave-guard.svelte';
import * as m from '$lib/paraglide/messages.js';

/**
 * 区画ごとに保存する編集ページの、保存中の区画・離れるときの確認・保存の後の知らせ (→ docs/ui.md「編集ページの区画」)。
 * 共有とユーザーの編集ページで同じ流れにする。
 *
 * コンポーネントの初期化の中で作ること (`LeaveGuard` が `beforeNavigate` を登録するため)。
 */
export class SectionSaveState<Part extends string> {
	/** 保存中の区画。保存ボタンの読み込み中の印に使う。 */
	savingPart = $state<Part | null>(null);
	readonly leaveGuard: LeaveGuard;
	#forms: FormDirtyState<unknown>[];
	#errorDialog: ErrorDialogState;

	constructor(forms: FormDirtyState<unknown>[], errorDialog: ErrorDialogState) {
		this.#forms = forms;
		this.#errorDialog = errorDialog;
		this.leaveGuard = new LeaveGuard(
			() => this.dirty,
			() => this.saving
		);
	}

	get saving(): boolean {
		return this.savingPart !== null;
	}

	/** どこかの区画に保存していない変更があるか。 */
	get dirty(): boolean {
		return this.#forms.some((form) => form.dirty);
	}

	/**
	 * 区画 `part` を保存する。`run` は保存できたら `true` を返す (失敗の理由は `run` の中でエラーダイアログに出す)。
	 * 保存できたら何を保存したか (`section`) を知らせて見出しを取り直し、保存中に止めた移動は、変更が残っていなければ続ける。
	 */
	async save(part: Part, section: string, run: () => Promise<boolean>): Promise<void> {
		if (this.saving) return;
		this.savingPart = part;
		let saved: boolean;
		try {
			saved = await run();
		} finally {
			this.savingPart = null;
		}
		if (saved) {
			toast.success(m.common_section_saved_toast({ section }));
			await refreshAll();
		}
		await this.leaveGuard.resumeAfterSave(() => this.dirty || this.#errorDialog.open);
	}

	/** 離れるときの確認で「破棄」を選んだときに、どの区画の変更も無かったことにする。 */
	discard(): void {
		for (const form of this.#forms) form.markPristine();
	}
}
