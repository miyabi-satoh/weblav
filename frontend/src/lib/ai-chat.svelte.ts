import { untrack } from 'svelte';

/**
 * 「AI に聞く」のパネルの状態 (→ docs/ai.md「簡易チャット」)。
 * パネルは `ai-chat.svelte` の1つだけで、ファイルと画像のビューアーの上部のバーからも開く。
 */

/** パネルの中身。ビューアーの「Esc で閉じる」をパネルの中では止めるのに、パネルの中かを確かめる。 */
export const AI_CHAT_PANEL_ID = 'ai-chat-panel';

/** 簡易チャットに付ける、ビューアーで見ているファイル。 */
export type AiChatFile = {
	/** 札と回答の上に出す名前。 */
	name: string;
	/** 中身を取る URL (ビューアーが開いている URL)。 */
	src: string;
	/** 中身を送るか。ビューアーで開いた動画と埋め込み (動画サイトなど) は名前だけを送る (file-viewer.svelte)。 */
	sendsContent: boolean;
};

/** ファイルを渡すビューアー。片方を閉じても、もう片方が渡したファイルを消さないように分ける。 */
type FileOwner = 'file' | 'image';

let available = $state(false);
let open = $state(false);
let checks = $state(0);
let file = $state<{ owner: FileOwner; file: AiChatFile } | null>(null);

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
		// $effect の中から呼ばれるので、読んだ値を依存にしない。依存にすると、書いた値で自分が回り直して止まらない。
		checks = untrack(() => checks) + 1;
	},
	/** ビューアーで見ているファイル。無ければ `null`。 */
	get file(): AiChatFile | null {
		return file?.file ?? null;
	},
	/** ビューアーが、見ているファイルを渡す。閉じたら `null` を渡す。ほかのビューアーが渡したものは消さない。 */
	setFile(owner: FileOwner, value: AiChatFile | null) {
		if (value !== null) file = { owner, file: value };
		else if (file?.owner === owner) file = null;
	},
	/** `target` がパネルの中か。 */
	contains(target: EventTarget | null): boolean {
		return target instanceof Node && !!document.getElementById(AI_CHAT_PANEL_ID)?.contains(target);
	}
};
