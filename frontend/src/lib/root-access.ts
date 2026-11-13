/**
 * 共有ごとの権限 (→ docs/shares.md「できること」) を、管理画面の共有の編集ページで選ぶための型と変換。
 */
import type { components } from '#lib/api/schema.js';

type ShareAccess = components['schemas']['ShareAccess'];
type UserListItem = components['schemas']['UserListItem'];
export type AccessException = components['schemas']['RootUserAccess'];

/**
 * 画面に並べる例外。管理者はどの共有でもすべての操作ができるので並べない。
 * 例外を持ったまま管理者に上げた人の分は、保存しても消さない (一般ユーザーに戻したときに元の設定へ戻すため)。
 */
export function visibleExceptions(
	exceptions: AccessException[],
	users: UserListItem[] | null
): AccessException[] {
	const admins = new Set(users?.filter((user) => user.role === 'admin').map((user) => user.id));
	return exceptions.filter((exception) => !admins.has(exception.userId));
}

/** 例外に足せる人。一般ユーザーのうち、まだ例外に無い人。 */
export function exceptionCandidates(
	users: UserListItem[] | null,
	exceptions: AccessException[]
): UserListItem[] {
	const added = new Set(exceptions.map((exception) => exception.userId));
	return (users ?? []).filter((user) => user.role === 'user' && !added.has(user.id));
}

/**
 * 例外に足す。サーバーと同じくユーザー名の順に並べる。足して外せば元の並びに戻るので、変更なしと判定できる。
 * 既定と同じ値も例外として残す (後で既定を変えても、その人だけは変わらないように選んだため)。
 */
export function addException(
	exceptions: AccessException[],
	user: UserListItem,
	access: ShareAccess
): AccessException[] {
	return [...exceptions, { userId: user.id, username: user.username, access }].sort((a, b) =>
		a.username.toLowerCase().localeCompare(b.username.toLowerCase())
	);
}
