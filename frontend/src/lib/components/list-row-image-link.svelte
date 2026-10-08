<script lang="ts">
	import type { Snippet } from 'svelte';
	import FileIcon from '@lucide/svelte/icons/file';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { browseRowClass, tileCornerClass } from '$lib/list-row';
	import { isPlainClick, type ViewerImage } from '$lib/image-viewer';
	import { openViewerItem, type ViewerItem } from '$lib/viewer-items';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import { Spinner } from '$lib/components/ui/spinner';
	import * as m from '$lib/paraglide/messages.js';

	// 閲覧側の一覧の、ページ内のビューアで開く画像の行・タイル (→ $lib/list-row.ts)。
	// 元の画像への直リンクのまま置き、修飾キー付きのクリックや長押しのメニューでは
	// ブラウザに任せて新しいタブで開けるようにする。
	let {
		image,
		items,
		compact = false,
		children
	}: {
		/** `items` と同じ組み立て方で作る (押した行を `items` の中から `src` で探すため)。 */
		image: ViewerImage;
		/** この行が並ぶ一覧の、ページ内で開くもの (表示順。→ $lib/viewer-items.ts)。ビューアの前・次になる。 */
		items: ViewerItem[];
		/** フォルダ一覧の詰めた行 (`browseRowClass(true)`) にする。タイルには効かない。 */
		compact?: boolean;
		/** アイコンの右に置く、行の文字。 */
		children: Snippet;
	} = $props();

	/** 元の画像を読み込んでいて、ビューアがまだ開いていない間。 */
	let opening = $state(false);

	/** 待っている間の、ビューアを開くのをやめさせる口。 */
	let pending: AbortController | undefined;

	// 待つ間に行が消えたり、別のページへ移って行が使い回されたりしたら、
	// 移った先の上に前の画像のビューアを開かない。
	$effect(() => {
		void image;
		void items;
		return () => pending?.abort();
	});

	function handleClick(event: MouseEvent) {
		if (!isPlainClick(event)) return;
		event.preventDefault();
		if (opening) return;
		opening = true;
		pending = new AbortController();
		void openViewerItem({ type: 'image', image }, items, pending.signal).finally(() => {
			opening = false;
			pending = undefined;
		});
	}
</script>

<!-- API への直リンク (→ $lib/api/urls.ts)。 -->
<a
	href={image.src}
	class={browseRowClass(compact)}
	target="_blank"
	rel="external noopener noreferrer"
	onclick={handleClick}
>
	<ListRowIcon
		icon={FileIcon}
		thumbnail={{ src: image.thumbnailSrc, original: image.src }}
		{compact}
	/>
	{@render children()}
	{#if opening}
		<!-- タイルは新規タブの印と同じ右上の隅に置く (画像のタイルには印が無いので重ならない)。 -->
		<Spinner
			class={['shrink-0 text-muted-foreground', browseLayout.tile ? tileCornerClass : 'ml-auto']}
			aria-label={m.common_loading()}
		/>
	{/if}
</a>
