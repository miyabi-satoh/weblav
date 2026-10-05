/**
 * 閲覧側の一覧 (ホーム・グループ・フォルダ・アーカイブ) の並べ方 (→ docs/ui.md「UI 全般」)。
 * 一覧すべてに1つの設定で効かせるので、ページ・コンポーネント単位ではなく
 * モジュールスコープの単一の $state で持つ。
 */
export type BrowseLayout = 'list' | 'tile';

/** 選んだ並べ方を端末に覚えさせるキー。 */
const STORAGE_KEY = 'weblav:browse-layout';

function readLayout(): BrowseLayout {
	// プライベートブラウズ等で localStorage が使えなくても、既定のリストとして動かす。
	try {
		return globalThis.localStorage?.getItem(STORAGE_KEY) === 'tile' ? 'tile' : 'list';
	} catch {
		return 'list';
	}
}

let layout = $state<BrowseLayout>(readLayout());

export const browseLayout = {
	get value(): BrowseLayout {
		return layout;
	},
	set value(next: BrowseLayout) {
		layout = next;
		try {
			globalThis.localStorage?.setItem(STORAGE_KEY, next);
		} catch {
			// 覚えられなくても、今の画面での切り替えは効かせる。
		}
	},
	get tile(): boolean {
		return layout === 'tile';
	}
};
