import { beforeNavigate, goto } from '$app/navigation';
import { leaveConfirmBypassed } from '$lib/leave-confirm';

/**
 * 保存していない変更を、別の画面へ移って失わないようにする (→ docs/ui.md「UI 全般」の離れるときの確認)。
 * 編集ページとサイト設定で同じ動きにするため、確かめ方をここに1つ持つ。画面は `leave-confirm-dialog.svelte` を置く。
 *
 * コンポーネントの初期化の中で作ること (`beforeNavigate` を登録するため)。
 */
export class LeaveGuard {
	/** 確認を出している移動の行き先。`null` なら確認を出していない。 */
	pending = $state<URL | null>(null);
	/** 保存中に移ろうとした行き先。保存が終わってから、結果に応じて移るか確認を出す。 */
	#afterSave: URL | null = null;
	#dirty: () => boolean;
	#saving: () => boolean;

	/**
	 * @param dirty どこかの区画に保存していない変更があるか。
	 * @param saving 保存中か。保存は、成功して変更が無くなった後の取り直しまで含めて待つ。
	 */
	constructor(dirty: () => boolean, saving: () => boolean = () => false) {
		this.#dirty = dirty;
		this.#saving = saving;
		beforeNavigate((navigation) => {
			if (!this.#dirty() && !this.#saving()) return;
			if (leaveConfirmBypassed()) return;
			// タブを閉じる・再読み込みは、ブラウザー標準の確認に任せる。
			if (navigation.type === 'leave') {
				navigation.cancel();
				return;
			}
			if (!navigation.to) return;
			navigation.cancel();
			if (this.#saving()) {
				this.#afterSave = navigation.to.url;
				return;
			}
			this.pending = navigation.to.url;
		});
	}

	/**
	 * 保存の後に呼ぶ。保存中に止めた移動を続ける。
	 * @param stay 移らずに留まるか (保存に失敗して変更が残った・エラーを読ませたいなど)。
	 */
	async resumeAfterSave(stay: () => boolean = () => this.#dirty()): Promise<void> {
		const target = this.#afterSave;
		this.#afterSave = null;
		if (target === null || stay()) return;
		// 行き先は SvelteKit のナビゲーションが解決済みの URL。
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		await goto(target);
	}

	/** 変更を捨てて、止めた移動を続ける。`discard` で区画の変更を無かったことにする (移動をもう一度止めないため)。 */
	async discardAndLeave(discard: () => void): Promise<void> {
		const target = this.pending;
		this.pending = null;
		if (target === null) return;
		discard();
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		await goto(target);
	}
}
