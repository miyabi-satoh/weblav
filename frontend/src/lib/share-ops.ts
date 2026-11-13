import type { components } from '#lib/api/schema.js';
import * as m from '#lib/paraglide/messages.js';

type ShareAccess = components['schemas']['ShareAccess'];

/**
 * 共有の中の、今いるフォルダー。共有の画面 (`/shares/[id]`) とフォルダーのコンテンツの画面
 * (`/folders/[id]` の `browse.share`) の両方から、同じ操作の部品を開くために持ち回す。
 */
export type ShareDir = {
	shareId: number;
	shareName: string;
	/** 共有からの相対パス。共有そのものなら空。 */
	path: string;
	/** この場所で、見ている人ができること。 */
	access: ShareAccess;
	/** 消したものを OS のごみ箱へ送れる場所か。 */
	trash: boolean;
	/** アップロードできる1ファイルの上限 (バイト)。分からない・上限が無ければ `null`。 */
	uploadMaxBytes: number | null;
};

/** 操作する1行。 */
export type ShareTarget = {
	name: string;
	/** 共有からの相対パス。 */
	path: string;
	isDir: boolean;
	/** ホームに載っているか。 */
	listed: boolean;
};

/** 行の ⋮ に出す操作。並べる順でもある。 */
export type ShareAction = 'rename' | 'move' | 'copy' | 'download' | 'listOnHome' | 'delete';

/** `access` が `needed` 以上か。 */
export function accessAtLeast(access: ShareAccess, needed: ShareAccess): boolean {
	return ACCESS_CHOICES.indexOf(access) >= ACCESS_CHOICES.indexOf(needed);
}

/** アップロード・新しいフォルダーを出すか。追加のみの共有でもできる (→ docs/ui.md「共有の中の画面」)。 */
export function canAddTo(access: ShareAccess): boolean {
	return accessAtLeast(access, 'add');
}

/**
 * 行の ⋮ に出す操作。押してから断られるものは出さない (→ docs/ui.md「共有の中の画面」)。
 * - 名前の変更・移動・削除は「フルアクセス」の場所だけ。
 * - コピーは読める場所から、足せる共有がどこかにあるときだけ (`copyDestinationExists`)。
 * - ダウンロードはファイルだけ。ホームに載せるのは、まだ載せていないものだけ。
 */
export function shareRowActions(
	access: ShareAccess,
	target: Pick<ShareTarget, 'isDir' | 'listed'>,
	copyDestinationExists: boolean
): ShareAction[] {
	const all = accessAtLeast(access, 'all');
	const actions: ShareAction[] = [];
	if (all) actions.push('rename', 'move');
	if (accessAtLeast(access, 'read') && copyDestinationExists) actions.push('copy');
	if (!target.isDir) actions.push('download');
	if (!target.listed) actions.push('listOnHome');
	if (all) actions.push('delete');
	return actions;
}

/** 共有の中の `dir` の直下にある `name` の、共有からの相対パス。 */
export function childSharePath(dir: string, name: string): string {
	return dir === '' ? name : `${dir}/${name}`;
}

/** 相対パスの最後の名前。共有そのもの (空) なら `fallback`。 */
export function lastSegment(path: string, fallback: string): string {
	return (
		path
			.split('/')
			.filter((segment) => segment !== '')
			.at(-1) ?? fallback
	);
}

/** 共有の中でできることの名前 (管理画面の権限・操作の記録)。 */
export function shareAccessLabel(access: ShareAccess): string {
	if (access === 'read') return m.shares_access_level_read();
	if (access === 'add') return m.shares_access_level_add();
	if (access === 'all') return m.shares_access_level_all();
	return m.shares_access_level_none();
}

/** 権限の弱い順 (サーバーの `ShareAccess` の順と同じ)。権限のダイアログで選ばせる順でもある。 */
export const ACCESS_CHOICES: readonly ShareAccess[] = ['none', 'read', 'add', 'all'];
