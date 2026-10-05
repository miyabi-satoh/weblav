import { LOCATION_SEPARATOR } from '$lib/root-location';

/** id から行を引く表。`selfAndAncestors` へ渡すために作る。 */
export function byIdMap<T extends { id: number }>(items: readonly T[]): ReadonlyMap<number, T> {
	return new Map(items.map((item) => [item.id, item]));
}

/**
 * `startId` のノード自身から、親方向に辿ったノードを近い順に返す。
 * `startId` が `byId` に無ければ空。
 *
 * 深さの上限は設けない。サーバー側の `validate_parent` が循環を拒むので、`parentId` を
 * 辿れば必ずルートに着く。通った id を覚えておくのは、データが壊れて循環していても
 * 無限ループにしないため (固定の深さ上限と違い、実際のネスト数を制限しない)。
 */
export function selfAndAncestors<T extends { id: number; parentId?: number | null }>(
	startId: number,
	byId: ReadonlyMap<number, T>
): T[] {
	const chain: T[] = [];
	const visited = new Set<number>();
	let current = byId.get(startId);
	while (current && !visited.has(current.id)) {
		visited.add(current.id);
		chain.push(current);
		current = current.parentId != null ? byId.get(current.parentId) : undefined;
	}
	return chain;
}

/**
 * 管理画面の一覧の並び。親の行の直後にその子を置き、字下げに使う深さを添えて返す
 * (→ docs/ui.md「UI 全般」)。同じ階層の中は渡された順 (API が返す登録順) のまま。
 *
 * 渡された行は必ず全部返す。親が一覧に無い行はルート直下として扱い、循環していて
 * ルートから辿り着けない行は最後にまとめて置く。一覧から消してしまうと、
 * 壊れたデータを管理画面から直せなくなるため。
 */
export function flattenTree<T extends { id: number; parentId?: number | null }>(
	contents: readonly T[]
): { content: T; depth: number }[] {
	const childrenOf = new Map<number | null, T[]>();
	const ids = new Set(contents.map((content) => content.id));
	for (const content of contents) {
		const parentId =
			content.parentId != null && ids.has(content.parentId) ? content.parentId : null;
		const siblings = childrenOf.get(parentId);
		if (siblings) siblings.push(content);
		else childrenOf.set(parentId, [content]);
	}

	const rows: { content: T; depth: number }[] = [];
	const visited = new Set<number>();
	const walk = (parentId: number | null, depth: number) => {
		for (const content of childrenOf.get(parentId) ?? []) {
			if (visited.has(content.id)) continue;
			visited.add(content.id);
			rows.push({ content, depth });
			walk(content.id, depth + 1);
		}
	};
	walk(null, 0);
	// ルートから辿り着けなかった行 (循環)。並びは崩れるが、消さずに出す。
	for (const content of contents) {
		if (visited.has(content.id)) continue;
		visited.add(content.id);
		rows.push({ content, depth: 0 });
	}
	return rows;
}

/** `startId` から親方向に辿ったタイトルを、ルートに近い順に ` / ` で繋ぐ。 */
export function ancestorPath<T extends { id: number; parentId?: number | null; title: string }>(
	startId: number,
	byId: ReadonlyMap<number, T>
): string {
	return selfAndAncestors(startId, byId)
		.reverse()
		.map((content) => content.title)
		.join(LOCATION_SEPARATOR);
}
