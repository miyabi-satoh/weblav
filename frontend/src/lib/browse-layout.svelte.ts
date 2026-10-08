import { readStored, writeStored } from '$lib/stored';

/**
 * 閲覧側の一覧 (ホーム・グループ・フォルダ・アーカイブ) の並べ方 (→ docs/ui.md「UI 全般」)。
 * 一覧すべてに1つの設定で効かせるので、ページ・コンポーネント単位ではなく
 * モジュールスコープの単一の $state で持つ。
 */
export type BrowseLayout = 'list' | 'tile';

/** 選んだ並べ方を端末に覚えさせるキー。 */
const STORAGE_KEY = 'weblav:browse-layout';

let layout = $state<BrowseLayout>(readStored(STORAGE_KEY) === 'tile' ? 'tile' : 'list');

export const browseLayout = {
	get value(): BrowseLayout {
		return layout;
	},
	set value(next: BrowseLayout) {
		layout = next;
		writeStored(STORAGE_KEY, next);
	},
	get tile(): boolean {
		return layout === 'tile';
	}
};
