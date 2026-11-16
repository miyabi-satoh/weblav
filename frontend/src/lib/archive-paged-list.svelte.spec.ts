import { describe, expect, it } from 'vitest';
import { ARCHIVE_CHUNK_SIZE, ArchivePagedList } from './archive-paged-list.svelte';

type Row = { id: number };

/** `ids` を並べた一覧の、`offset` 行目からの区切り。 */
function chunkOf(ids: number[], offset: number): Row[] {
	return ids.slice(offset, offset + ARCHIVE_CHUNK_SIZE).map((id) => ({ id }));
}

function firstOf(ids: number[], snapshot: string) {
	return { snapshot, total: ids.length, items: chunkOf(ids, 0) };
}

const range = (length: number, from = 1) => Array.from({ length }, (_, i) => from + i);

describe('ArchivePagedList', () => {
	it('retries a chunk that failed once when asked to', async () => {
		const ids = range(ARCHIVE_CHUNK_SIZE * 2);
		let calls = 0;
		const list = new ArchivePagedList<Row>(firstOf(ids, 's1'), async (_, offset) => {
			calls += 1;
			if (calls === 1) throw new Error('network');
			return { items: chunkOf(ids, offset), stale: false };
		});

		await list.ensureLoaded(ARCHIVE_CHUNK_SIZE);
		expect(list.get(ARCHIVE_CHUNK_SIZE)).toBeUndefined();
		await list.ensureLoaded(ARCHIVE_CHUNK_SIZE);
		expect(calls).toBe(1);

		await list.ensureLoaded(ARCHIVE_CHUNK_SIZE, true);
		expect(list.get(ARCHIVE_CHUNK_SIZE)).toEqual({ id: ARCHIVE_CHUNK_SIZE + 1 });
	});

	it('renews an expired list and continues after the last row it gave out', async () => {
		const before = range(ARCHIVE_CHUNK_SIZE * 2);
		// 取り直す間に、先頭にファイルが2つ増えた。
		const after = [9001, 9002, ...before];
		const fetch = async (snapshot: string, offset: number) =>
			snapshot === 's1'
				? { expired: true as const }
				: { items: chunkOf(after, offset), stale: false };
		const list = new ArchivePagedList<Row>(firstOf(before, 's1'), fetch, undefined, async () =>
			firstOf(after, 's2')
		);

		await list.ensureLoaded(ARCHIVE_CHUNK_SIZE);
		expect(list.expired).toBe(true);

		// 先頭の区切りの終わりまで並べた後。最後に並べたのは 500 番の行。
		const renewed = await list.renewedFrom(ARCHIVE_CHUNK_SIZE, (row) => row.id);
		expect(renewed?.list.snapshot).toBe('s2');
		expect(renewed?.next).toBe(ARCHIVE_CHUNK_SIZE + 2);
		expect(renewed?.list.get(renewed.next - 1)).toEqual({ id: ARCHIVE_CHUNK_SIZE });
	});

	it('cannot renew a list without a way to fetch it again', async () => {
		const list = new ArchivePagedList<Row>(firstOf(range(3), 's1'), async () => ({
			expired: true
		}));
		expect(await list.renewedFrom(1, (row) => row.id)).toBeNull();
	});
});
