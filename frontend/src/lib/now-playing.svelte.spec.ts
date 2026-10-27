import { afterEach, describe, expect, it } from 'vitest';
import { nowPlaying, type Track, type TrackSource } from '$lib/now-playing.svelte';

function tracks(count: number): Track[] {
	return Array.from({ length: count }, (_, i) => ({ src: `/t${i}`, title: `t${i}`, position: i }));
}

/** 今の曲から「次」を押していった曲の並び。端で止まるか、`limit` 曲で打ち切る。 */
function playedOrder(limit: number): string[] {
	const order = [nowPlaying.current!.src];
	while (order.length < limit && nowPlaying.hasNext) {
		nowPlaying.next();
		order.push(nowPlaying.current!.src);
	}
	return order;
}

describe('nowPlaying', () => {
	afterEach(() => {
		nowPlaying.stop();
		nowPlaying.shuffle = false;
		nowPlaying.repeat = 'off';
	});

	it('stops at the last track unless repeating all, which wraps both ways', () => {
		const list = tracks(3);
		nowPlaying.play(list[2], list);
		expect(nowPlaying.hasNext).toBe(false);

		nowPlaying.repeat = 'all';
		nowPlaying.next();
		expect(nowPlaying.current?.src).toBe('/t0');
		nowPlaying.previous();
		expect(nowPlaying.current?.src).toBe('/t2');
	});

	it('does not wrap while the rest of the list is still to be fetched', () => {
		const list = tracks(2);
		const pending: TrackSource = { more: () => new Promise<Track[]>(() => {}) };
		nowPlaying.repeat = 'all';
		nowPlaying.play(list[1], list, pending);
		expect(nowPlaying.hasNext).toBe(false);
	});

	it('shuffles the rest of a list after the current track and restores the order when turned off', () => {
		const list = tracks(20);
		nowPlaying.play(list[5], list);
		nowPlaying.shuffle = true;

		const order = playedOrder(20);
		expect(order[0]).toBe('/t5');
		expect([...order].sort()).toEqual(list.map((t) => t.src).sort());
		const others = list.filter((t) => t.src !== '/t5').map((t) => t.src);
		expect(order.slice(1)).not.toEqual(others);

		nowPlaying.shuffle = false;
		const current = nowPlaying.current!;
		const rest = playedOrder(20);
		expect(rest).toEqual(list.slice(current.position).map((t) => t.src));
	});

	it('takes the shuffled order from the list when it can shuffle, and resumes after the current track when turned off', async () => {
		const list = tracks(10);
		const shuffled = [list[7], list[2], list[3]];
		const after: Track[] = [];
		const source: TrackSource = {
			more: async () => [],
			shuffled: () => {
				let sent = false;
				return {
					more: async () => {
						if (sent) return [];
						sent = true;
						return shuffled;
					}
				};
			},
			after: (track) => {
				after.push(track);
				return { more: async () => list.slice((track.position ?? 0) + 1) };
			}
		};
		nowPlaying.shuffle = true;
		nowPlaying.play(list[0], list.slice(0, 2), source);
		await expect.poll(() => nowPlaying.hasNext).toBe(true);
		expect(playedOrder(10)).toEqual(['/t0', '/t7', '/t2', '/t3']);

		nowPlaying.shuffle = false;
		expect(after.map((t) => t.src)).toEqual(['/t3']);
		await expect.poll(() => nowPlaying.hasNext).toBe(true);
		expect(playedOrder(10)).toEqual(['/t3', '/t4', '/t5', '/t6', '/t7', '/t8', '/t9']);
	});

	it('shuffles the loaded tracks when the list can no longer make a shuffled order', async () => {
		const list = tracks(5);
		const source: TrackSource = {
			more: async () => [],
			shuffled: () => ({ more: async () => [] })
		};
		nowPlaying.shuffle = true;
		nowPlaying.play(list[0], list, source);
		await expect.poll(() => nowPlaying.hasNext).toBe(true);
		expect([...playedOrder(10)].sort()).toEqual(list.map((t) => t.src).sort());
	});

	it('keeps tracks fetched in order while shuffle was turned on', async () => {
		const list = tracks(6);
		let resolve: (tracks: Track[]) => void = () => {};
		let calls = 0;
		const source: TrackSource = {
			more: () => {
				calls += 1;
				return calls === 1 ? new Promise<Track[]>((r) => (resolve = r)) : Promise.resolve([]);
			}
		};
		nowPlaying.play(list[0], list.slice(0, 2), source);
		nowPlaying.shuffle = true;
		resolve(list.slice(2));
		await Promise.resolve();
		nowPlaying.shuffle = false;
		await expect.poll(() => nowPlaying.hasNext).toBe(true);
		expect(playedOrder(10)).toEqual(list.map((t) => t.src));
	});
});
