/**
 * 「AI に聞く」のパネルの状態 (→ docs/ai.md「簡易チャット」)。
 * パネルは `ai-chat.svelte` の1つだけで、ファイルと画像のビューアーの上部のバーからも開く。
 */

/** パネルの中身。ビューアーの「Esc で閉じる」をパネルの中では止めるのに、パネルの中かを確かめる。 */
export const AI_CHAT_PANEL_ID = 'ai-chat-panel';

let available = $state(false);
let open = $state(false);
let checks = $state(0);

export const aiChat = {
	/** この人が AI を使えるか。使えないときは、開くボタンをどこにも出さない。 */
	get available() {
		return available;
	},
	set available(value: boolean) {
		available = value;
		if (!value) open = false;
	},
	get open() {
		return open;
	},
	set open(value: boolean) {
		open = value && available;
	},
	/** 使えるかを確かめ直した回数。読むと、確かめ直しを頼まれたときに読み直せる。 */
	get checks() {
		return checks;
	},
	/** 使えるかを確かめ直す。コンピューターの登録が変わったときに呼ぶ。 */
	recheck() {
		checks += 1;
	},
	/** `target` がパネルの中か。 */
	contains(target: EventTarget | null): boolean {
		return target instanceof Node && !!document.getElementById(AI_CHAT_PANEL_ID)?.contains(target);
	}
};
