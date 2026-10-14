import { afterEach, describe, expect, it } from 'vitest';
import { masonry } from './masonry';

describe('masonry', () => {
	let cleanup: (() => void) | void;
	let grid: HTMLElement;

	afterEach(() => {
		cleanup?.();
		grid.remove();
	});

	function mount(heights: number[]) {
		grid = document.createElement('div');
		grid.style.display = 'grid';
		grid.style.gridTemplateColumns = '100px 100px';
		grid.style.columnGap = '10px';
		grid.style.width = '210px';
		for (const height of heights) {
			const item = document.createElement('div');
			item.style.height = `${height}px`;
			grid.append(item);
		}
		document.body.append(grid);
		cleanup = masonry(grid);
	}

	/** ResizeObserver の知らせは描画の後に届くので、2フレーム待つ。 */
	async function settle() {
		for (let i = 0; i < 2; i++) await new Promise((resolve) => requestAnimationFrame(resolve));
	}

	function boxes() {
		const top = grid.getBoundingClientRect().top;
		const left = grid.getBoundingClientRect().left;
		return [...grid.children].map((child) => {
			const rect = child.getBoundingClientRect();
			return { x: rect.left - left, y: rect.top - top };
		});
	}

	it('puts each item under the shortest column with the column gap between items', async () => {
		mount([300, 100, 50, 80]);
		await settle();
		expect(boxes()).toEqual([
			{ x: 0, y: 0 },
			{ x: 110, y: 0 },
			// 右の列 (100px) が短いので右へ。
			{ x: 110, y: 110 },
			// 右の列は 100 + 10 + 50 = 160px で、まだ左 (300px) より短い。
			{ x: 110, y: 170 }
		]);
	});

	it('moves the following items when an item grows', async () => {
		mount([100, 200, 50]);
		await settle();
		expect(boxes()[2]).toEqual({ x: 0, y: 110 });
		(grid.children[0] as HTMLElement).style.height = '300px';
		await settle();
		expect(boxes()[2]).toEqual({ x: 110, y: 210 });
	});
});
