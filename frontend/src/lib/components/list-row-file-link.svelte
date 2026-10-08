<script lang="ts">
	import type { Snippet } from 'svelte';
	import FileIcon from '@lucide/svelte/icons/file';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import { isPlainClick } from '$lib/image-viewer';
	import { openViewerItem, type ViewerItem } from '$lib/viewer-items';
	import { browseRowClass } from '$lib/list-row';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';

	// 閲覧側の一覧の、ページ内のビューアで開く PDF・動画・テキストなどの行・タイル (→ $lib/list-row.ts)。
	// 画像の行 (list-row-image-link.svelte) と同じく、元のファイルへの直リンクのまま置き、
	// 修飾キー付きのクリックや長押しのメニューではブラウザに任せて新しいタブで開けるようにする。
	let {
		file,
		items,
		compact = false,
		children
	}: {
		/** `items` と同じ組み立て方で作る (押した行を `items` の中から `src` で探すため)。 */
		file: ViewerFile;
		/** この行が並ぶ一覧の、ページ内で開くもの (表示順。→ $lib/viewer-items.ts)。ビューアの前・次になる。 */
		items: ViewerItem[];
		/** フォルダ一覧の詰めた行 (`browseRowClass(true)`) にする。タイルには効かない。 */
		compact?: boolean;
		/** アイコンの右に置く、行の文字。 */
		children: Snippet;
	} = $props();

	function handleClick(event: MouseEvent) {
		if (!isPlainClick(event)) return;
		event.preventDefault();
		void openViewerItem({ type: 'file', file }, items, new AbortController().signal);
	}
</script>

<!-- API への直リンクか、URL のファイルの元の URL (→ $lib/api/urls.ts)。
     中継の URL は Office をダウンロードにし、LAN を指す名前では開けないため、元の URL にする。 -->
<a
	href={file.originalUrl ?? file.src}
	class={browseRowClass(compact)}
	target="_blank"
	rel="external noopener noreferrer"
	onclick={handleClick}
>
	<ListRowIcon icon={FileIcon} {compact} thumbnail={file.thumbnail} />
	{@render children()}
</a>
