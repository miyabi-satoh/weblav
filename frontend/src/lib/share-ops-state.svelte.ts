import type { components } from '$lib/api/schema';
import { accessAtLeast, type ShareDir, type ShareTarget } from '$lib/share-ops';

type ShareResponse = components['schemas']['ShareResponse'];
type TransferOp = components['schemas']['TransferOp'];

/** 開いているダイアログ。1度に1つだけ開く。 */
export type ShareDialog =
	| { kind: 'upload' }
	| { kind: 'newFolder' }
	| { kind: 'rename'; target: ShareTarget }
	| { kind: 'transfer'; op: TransferOp; target: ShareTarget }
	| { kind: 'delete'; target: ShareTarget }
	| { kind: 'listOnHome'; target: ShareTarget };

/**
 * 共有の中の操作 (→ docs/ui.md「共有の中の画面」) を、共有の画面とフォルダーのコンテンツの画面で同じ部品にするための状態。
 * 画面は、今いる場所と使える共有を渡し、⋮ や「追加」から `open()` する。ダイアログは `share-ops-host.svelte` が出す。
 */
export class ShareOpsState {
	dialog = $state<ShareDialog | null>(null);

	/** 今いる場所。 */
	readonly dir: () => ShareDir;
	/** 見ている人が使える共有。コピー・移動の置き先の候補。 */
	readonly shares: () => ShareResponse[];
	/** 中身が変わった後の画面の更新 (一覧を取り直す)。 */
	readonly onchanged: () => Promise<void>;

	constructor(dir: () => ShareDir, shares: () => ShareResponse[], onchanged: () => Promise<void>) {
		this.dir = dir;
		this.shares = shares;
		this.onchanged = onchanged;
	}

	open(dialog: ShareDialog) {
		this.dialog = dialog;
	}

	close() {
		this.dialog = null;
	}

	/** 足せる共有がどこかにあるか。無ければコピーを出さない (置き先を選べないため)。 */
	get copyDestinationExists(): boolean {
		return this.shares().some((share) => accessAtLeast(share.access, 'add'));
	}
}
