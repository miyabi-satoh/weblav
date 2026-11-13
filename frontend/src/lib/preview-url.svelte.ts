/**
 * ページ内で開いているもの (画像・ファイルのビューアー) を URL の `preview` に写し、読み込み直し・戻る・進むで
 * 開き直す・閉じる (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
 *
 * - 開いたら、履歴を1段 (shallow routing の pushState) 足す。ブラウザーの戻るで閉じられるように。
 * - ビューアーの中で前後に移ったら、置き換える。前後に移るたびに戻るの段を増やさないため。
 * - 閉じたら、足した段の上なら戻り、そうでなければ (読み込み直した後など) `preview` を外して置き換える。
 *
 * ビューアーを開く・閉じる手立ては持たず、`followPreviewUrl` に渡してもらう (→ file-viewer.svelte)。
 */

import { untrack } from 'svelte';
import { pushState, replaceState } from '$app/navigation';
import { page } from '$app/state';
import { previewHref, wantedPreviewKey } from '$lib/preview-query';
import type { ViewerItem } from '$lib/viewer-items';

/**
 * 前後に移ってから URL に写すまでの間。Safari は短い間に何度も履歴を書き換えると例外を投げ、
 * 左右キーを押し続けると移るたびに書くことになるため、止まってから写す (一覧の絞り込みと同じ間)。
 */
const MOVE_WRITE_DELAY_MS = 300;

/** 一覧が `key` の行を探す口。見つかれば、行から開いたときと同じ前後の並びと位置を返す。 */
export type PreviewSource = (
	key: string
) =>
	| { items: ViewerItem[]; index: number }
	| undefined
	| Promise<{ items: ViewerItem[]; index: number } | undefined>;

/** どちらかのビューアーが開いている。 */
let open = false;
/** 開いているものの key (URL に写す前のものを含む)。 */
let shownKey: string | null = null;
/** 前に読んだ・書いた URL の key。変わったときだけ受け取る (自分で書いた分に応じないため)。 */
let seen: string | null | undefined;
let pendingWrite: ReturnType<typeof setTimeout> | undefined;
/** 開き直したい key。一覧が揃ってから探す。 */
let restoring: string | null = null;
let restoreScheduled = false;
// 画面には出さないので、変わったことを知らせる SvelteSet にしない。
// eslint-disable-next-line svelte/prefer-svelte-reactivity
const sources = new Set<PreviewSource>();
let viewers: { open: (items: ViewerItem[], index: number) => void; close: () => void } | undefined;

function write(key: string | null, push: boolean) {
	seen = key;
	const href = previewHref(location.pathname, location.search, key);
	if (push) {
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- 今の URL のパスをそのまま使う (→ AGENTS.md「コードの規約」)
		pushState(href, { ...page.state, preview: key, previewPushed: true });
	} else {
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- 今の URL のパスをそのまま使う (→ AGENTS.md「コードの規約」)
		replaceState(href, { ...page.state, preview: key });
	}
}

function cancelPendingWrite() {
	clearTimeout(pendingWrite);
	pendingWrite = undefined;
}

/**
 * ビューアーが `key` を見せ始めた (開いた・前後に移った・相手のビューアーへ渡った)。
 * `key` が無いもの (Markdown のリンクから開いた、一覧に無いファイル) は、URL から外す。
 */
export function previewShown(key: string | null) {
	if (!open) {
		open = true;
		shownKey = key;
		cancelPendingWrite();
		// 段を足すのは、行から開いたときだけ。読み込み直し・進むで開き直したときは、URL がもう `preview` を持つ
		// (アーカイブで位置の目安がずれていたときは、key を直すだけにする)。
		const wanted = wantedPreviewKey(page.state, page.url);
		if (key !== null && key !== wanted) write(key, wanted === null);
		return;
	}
	if (key === shownKey) return;
	shownKey = key;
	cancelPendingWrite();
	pendingWrite = setTimeout(() => {
		pendingWrite = undefined;
		if (open) write(shownKey, false);
	}, MOVE_WRITE_DELAY_MS);
}

/** 見ている人がビューアーを閉じた (閉じるボタン・Esc など)。画面の移動や戻るで閉じたときは呼ばない。 */
export function previewClosed() {
	if (!open) return;
	open = false;
	shownKey = null;
	cancelPendingWrite();
	if (page.state.previewPushed) {
		// 足した段を戻す。進むで開き直せるように、消さずに戻る。
		seen = null;
		history.back();
	} else {
		write(null, false);
	}
}

/**
 * 画面を移る (戻る・進むで別の画面へ移るのを含む)。ビューアーを閉じるかを返す。
 * ページを離れる (読み込み直す・外のサイトへ移る) ときは閉じない。ブラウザーがページを残して戻ってきたとき、URL と食い違わないように。
 * 書きかけは、戻ってきたときに同じものを開けるよう、移る前に今の画面の履歴へ書く。
 * 戻る・進むでは履歴がもう移った先を指しているので、書かずに取り消す。
 */
export function previewLeaving(navigation: { type: string; willUnload: boolean }): boolean {
	if (pendingWrite !== undefined) {
		cancelPendingWrite();
		if (navigation.type !== 'popstate' && open) write(shownKey, false);
	}
	if (navigation.willUnload) return false;
	open = false;
	shownKey = null;
	restoring = null;
	return true;
}

/**
 * URL の key をビューアーに合わせる。画面に1つだけ置くビューアー (file-viewer.svelte) が作るときに呼ぶ。
 * 戻るで `preview` が外れたら閉じ、読み込み直し・共有されたリンク・進むで `preview` が付いたら、一覧から探して開く。
 */
export function followPreviewUrl(handlers: NonNullable<typeof viewers>) {
	viewers = handlers;
	$effect(() => {
		const wanted = wantedPreviewKey(page.state, page.url);
		untrack(() => {
			if (wanted === seen) return;
			seen = wanted;
			if (open && wanted === shownKey) return;
			if (open) {
				open = false;
				shownKey = null;
				cancelPendingWrite();
			}
			// 開いていなくても閉じる。進むで開き直している途中 (画像を読む間) に戻ると、
			// 履歴の移動が shallow で beforeNavigate を通らず、ほかに取りやめる所が無いため。
			handlers.close();
			restoring = wanted;
			if (wanted !== null) scheduleRestore();
		});
	});
}

/**
 * 一覧が、`preview` の key を探す口を置く。一覧の部品が作るときに呼ぶ。
 * 外れていたら (ファイルが消えた・絞り込みで外れた) 開かず、URL から `preview` を外す。
 */
export function providePreviewSource(source: PreviewSource) {
	$effect(() => {
		sources.add(source);
		if (restoring !== null) scheduleRestore();
		return () => sources.delete(source);
	});
}

/** 同じ時に作られる一覧 (検索の結果は3つ並ぶ) が揃ってから探すよう、今の描画が終わるまで待つ。 */
function scheduleRestore() {
	if (restoreScheduled) return;
	restoreScheduled = true;
	setTimeout(() => {
		restoreScheduled = false;
		void restore();
	});
}

async function restore() {
	const key = restoring;
	if (key === null) return;
	for (const source of [...sources]) {
		const found = await source(key);
		// 探している間に戻る・ほかの行を開くなどで、開き直さなくてよくなった。
		if (restoring !== key) return;
		if (open) {
			restoring = null;
			return;
		}
		if (found) {
			restoring = null;
			viewers?.open(found.items, found.index);
			return;
		}
	}
	restoring = null;
	if (seen === key && !open) write(null, false);
}
