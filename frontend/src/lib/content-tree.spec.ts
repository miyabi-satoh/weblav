import { describe, expect, it } from 'vitest';
import { flattenTree, selfAndAncestors } from './content-tree';

type Node = { id: number; parentId?: number | null };

function mapById(nodes: Node[]): Map<number, Node> {
	return new Map(nodes.map((node) => [node.id, node]));
}

const ids = (nodes: Node[]) => nodes.map((node) => node.id);

describe('selfAndAncestors', () => {
	it('walks from the start node up to the root, nearest first', () => {
		const tree = mapById([
			{ id: 1, parentId: null },
			{ id: 2, parentId: 1 },
			{ id: 3, parentId: 2 }
		]);
		expect(ids(selfAndAncestors(3, tree))).toEqual([3, 2, 1]);
	});

	it('treats an omitted parentId as the root', () => {
		expect(ids(selfAndAncestors(1, mapById([{ id: 1 }])))).toEqual([1]);
	});

	it('returns nothing when the start node does not exist', () => {
		expect(selfAndAncestors(9, mapById([{ id: 1 }]))).toEqual([]);
	});

	it('stops at a parent that is missing from the map', () => {
		const tree = mapById([{ id: 2, parentId: 1 }]);
		expect(ids(selfAndAncestors(2, tree))).toEqual([2]);
	});

	// サーバー側で循環は拒まれるが、壊れたデータでも止まること。
	it('stops instead of looping forever on a cycle', () => {
		const tree = mapById([
			{ id: 1, parentId: 3 },
			{ id: 2, parentId: 1 },
			{ id: 3, parentId: 2 }
		]);
		expect(ids(selfAndAncestors(1, tree))).toEqual([1, 3, 2]);
	});
});

describe('flattenTree', () => {
	const rows = (nodes: Node[]) =>
		flattenTree(nodes).map(({ content, depth }) => `${'-'.repeat(depth)}${content.id}`);

	it('puts each child right under its parent, keeping the given order', () => {
		expect(
			rows([
				{ id: 1, parentId: null },
				{ id: 2, parentId: null },
				{ id: 3, parentId: 1 },
				{ id: 4, parentId: 3 },
				{ id: 5, parentId: 1 }
			])
		).toEqual(['1', '-3', '--4', '-5', '2']);
	});

	it('treats a row whose parent is missing as a root', () => {
		expect(rows([{ id: 2, parentId: 9 }])).toEqual(['2']);
	});

	// サーバー側で循環は拒まれるが、壊れたデータでも行が消えず、止まること。
	it('stops instead of looping forever on a cycle', () => {
		const flat = flattenTree([
			{ id: 1, parentId: 2 },
			{ id: 2, parentId: 1 }
		]);
		expect(flat.map(({ content }) => content.id).sort()).toEqual([1, 2]);
	});
});
