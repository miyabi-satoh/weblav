/**
 * アーカイブの一覧の要求のクエリと、区切りの続きの取り方 (→ docs/archive.md「一覧の API を区切って返す」)。
 * 閲覧の画面と管理画面のアイテムのタブで同じ形にする。
 */

import { client } from '$lib/api/client';
import type { components } from '$lib/api/schema';
import type { ChunkFetch, ShuffleFetch } from '$lib/archive-paged-list.svelte';
import { ARCHIVE_CHUNK_SIZE } from '$lib/archive-paged-list.svelte';
import { LIST_FILTER_QUERY } from '$lib/list-filter';

type ArchiveViewItem = components['schemas']['ArchiveViewItem'];
type ManageArchiveViewItem = components['schemas']['ManageArchiveViewItem'];

/**
 * 画面の URL の軸の絞り込み (`?科目=国語`)。共有されたリンクの形は変えないので、画面の URL は軸の名前をそのままキーにする。
 * 並び順 (`sort`)・名前の語 (`filter`)・公開の状態 (`status`) は軸でない。
 */
export function axisFilters(url: URL, reserved: string[] = []): Record<string, string> {
	const skip = new Set(['sort', LIST_FILTER_QUERY, ...reserved]);
	return Object.fromEntries([...url.searchParams].filter(([key]) => !skip.has(key)));
}

/**
 * サーバーへ送るクエリ。軸の絞り込みは `axis.<軸名>` にして、区切りの引数などと取り違えないようにする
 * (→ docs/archive.md「一覧の API を区切って返す」)。空の値は送らない。
 */
export function archiveListQuery(
	filters: Record<string, string>,
	extra: Record<string, string>
): Record<string, string> {
	const query: Record<string, string> = {};
	for (const [name, value] of Object.entries(filters)) query[`axis.${name}`] = value;
	for (const [key, value] of Object.entries(extra)) if (value.trim() !== '') query[key] = value;
	return query;
}

/** 閲覧用の一覧の、区切りの続きの取り方。 */
export function viewChunks(id: number): ChunkFetch<ArchiveViewItem> {
	return async (snapshot, offset, signal) => {
		const { data, response } = await client.GET('/api/v1/contents/{id}/archive/page', {
			params: { path: { id }, query: { snapshot, offset, limit: ARCHIVE_CHUNK_SIZE } },
			signal
		});
		if (response.status === 409) return { expired: true };
		if (!data) throw new Error(`archive page failed: ${response.status}`);
		return data;
	};
}

/** 管理用の一覧の、区切りの続きの取り方。 */
export function manageChunks(id: number): ChunkFetch<ManageArchiveViewItem> {
	return async (snapshot, offset, signal) => {
		const { data, response } = await client.GET('/api/v1/contents/{id}/manage-archive/page', {
			params: { path: { id }, query: { snapshot, offset, limit: ARCHIVE_CHUNK_SIZE } },
			signal
		});
		if (response.status === 409) return { expired: true };
		if (!data) throw new Error(`manage archive page failed: ${response.status}`);
		return data;
	};
}

/** 閲覧用の一覧の、音声の行を混ぜた結果の作り方 (→ docs/archive.md「音声のシャッフル」)。 */
export function viewShuffle(id: number): ShuffleFetch<ArchiveViewItem> {
	return async (snapshot) => {
		const { data, response } = await client.GET('/api/v1/contents/{id}/archive/shuffle', {
			params: { path: { id }, query: { snapshot, limit: ARCHIVE_CHUNK_SIZE } }
		});
		if (response.status === 409) return { expired: true };
		if (!data) throw new Error(`archive shuffle failed: ${response.status}`);
		return data;
	};
}

/** 管理用の一覧の、音声の行を混ぜた結果の作り方。 */
export function manageShuffle(id: number): ShuffleFetch<ManageArchiveViewItem> {
	return async (snapshot) => {
		const { data, response } = await client.GET('/api/v1/contents/{id}/manage-archive/shuffle', {
			params: { path: { id }, query: { snapshot, limit: ARCHIVE_CHUNK_SIZE } }
		});
		if (response.status === 409) return { expired: true };
		if (!data) throw new Error(`manage archive shuffle failed: ${response.status}`);
		return data;
	};
}
