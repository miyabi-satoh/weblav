/**
 * 共有ごとの権限 (→ docs/shares.md「できること」) を、管理画面の共有の編集ページで選ぶための型と変換。
 */
import type { components } from '$lib/api/schema';

type ShareAccess = components['schemas']['ShareAccess'];
type Root = components['schemas']['RootResponse'];
type UserListItem = components['schemas']['UserListItem'];

/** ユーザーごとの選択。`default` は例外を置かない (既定に従う)。 */
export type UserChoice = ShareAccess | 'default';

/** 一般ユーザーごとの今の選択。例外の無い人は `default`。管理者はどの共有でもすべての操作ができるので並べない。 */
export function accessChoices(root: Root, users: UserListItem[]): Record<number, UserChoice> {
	const choices: Record<number, UserChoice> = {};
	for (const user of users) if (user.role === 'user') choices[user.id] = 'default';
	for (const exception of root.userAccess) choices[exception.userId] = exception.access;
	return choices;
}

/** 送る例外。既定と同じ値も例外として残す (後で既定を変えても、その人だけは変わらないように選んだため)。 */
export function accessExceptions(
	choices: Record<number, UserChoice>
): { userId: number; access: ShareAccess }[] {
	return Object.entries(choices)
		.filter((entry): entry is [string, ShareAccess] => entry[1] !== 'default')
		.map(([userId, access]) => ({ userId: Number(userId), access }));
}
