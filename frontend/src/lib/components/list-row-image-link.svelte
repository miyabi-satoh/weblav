<script lang="ts">
	import type { Snippet } from 'svelte';
	import FileIcon from '@lucide/svelte/icons/file';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import {
		browseRowClass,
		browseRowPressClass,
		browseRowTextClass,
		tileCornerClass
	} from '$lib/list-row';
	import { isPlainClick, type ViewerImage } from '$lib/image-viewer';
	import { openViewerItem, type ViewerItem } from '$lib/viewer-items';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import { Spinner } from '$lib/components/ui/spinner';
	import * as m from '$lib/paraglide/messages.js';

	// 閲覧側の一覧の、ページ内のビューアーで開く画像の行・タイル (→ $lib/list-row.ts)。
	// 元の画像への直リンクのまま置き、修飾キー付きのクリックや長押しのメニューでは
	// ブラウザーに任せて新しいタブで開けるようにする。
	let {
		image,
		items,
		compact = false,
		children,
		subtitle,
		trailing
	}: {
		/** `items` と同じ組み立て方で作る (押した行を `items` の中から `src` で探すため)。 */
		image: ViewerImage;
		/** この行が並ぶ一覧の、ページ内で開くもの (表示順。→ $lib/viewer-items.ts)。ビューアーの前・次になる。 */
		items: ViewerItem[];
		/** フォルダー一覧の詰めた行 (`browseRowClass(true)`) にする。タイルには効かない。 */
		compact?: boolean;
		/** 行のタイトル。これを包むリンクの押せる範囲が、行いっぱいに広がる。 */
		children: Snippet;
		/** タイトルの下に置く2段目。 */
		subtitle?: Snippet;
		/** 行の文字の後ろに置くもの。 */
		trailing?: Snippet;
	} = $props();

	/** 元の画像を読み込んでいて、ビューアーがまだ開いていない間。 */
	let opening = $state(false);

	/** 待っている間の、ビューアーを開くのをやめさせる口。 */
	let pending: AbortController | undefined;

	// 待つ間に行が消えたり、別のページへ移って行が使い回されたりしたら、
	// 移った先の上に前の画像のビューアーを開かない。
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

<div class={browseRowClass(compact)}>
	<ListRowIcon
		icon={FileIcon}
		thumbnail={{ src: image.thumbnailSrc, original: image.src }}
		{compact}
	/>
	<span class={browseRowTextClass()}>
		<!-- API への直リンク (→ $lib/api/urls.ts)。 -->
		<a
			href={image.src}
			class={browseRowPressClass()}
			target="_blank"
			rel="external noopener noreferrer"
			onclick={handleClick}
		>
			{@render children()}
		</a>
		{@render subtitle?.()}
	</span>
	{@render trailing?.()}
	{#if opening}
		<!-- タイルは新規タブの印と同じ右上の隅に置く (画像のタイルには印が無いので重ならない)。 -->
		<Spinner
			class={['shrink-0 text-muted-foreground', browseLayout.tile ? tileCornerClass : 'ml-auto']}
			aria-label={m.common_loading()}
		/>
	{/if}
</div>
