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
		/**
		 * この行が並ぶ一覧の、ページ内で開くもの (表示順。→ $lib/viewer-items.ts)。ビューアーの前・次になる。
		 * 関数なら押したときに呼ぶ (区切って取る一覧は、前後の区切りを取ってから作る)。
		 */
		items: ViewerItem[] | (() => Promise<ViewerItem[]>);
		/** フォルダー一覧の詰めた行 (`browseRowClass(true)`) にする。タイルには効かない。 */
		compact?: boolean;
		/** 行のタイトル。これを包むリンクの押せる範囲が、行いっぱいに広がる。 */
		children: Snippet;
		/** タイトルの下に置く2段目。 */
		subtitle?: Snippet;
		/** 行の右端に置くもの (「作成者のみ」の印・⋮ のメニュー)。 */
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
		const signal = pending.signal;
		void (typeof items === 'function' ? items() : Promise.resolve(items))
			.then((list) => openViewerItem({ type: 'image', image }, list, signal))
			.finally(() => {
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
	{#if opening}
		<!-- ⋮ のメニューが行の右端・タイルの右上を使うので、その手前・左上の隅に置く。 -->
		<Spinner
			class={['shrink-0 text-muted-foreground', browseLayout.tile ? tileCornerClass : 'ml-auto']}
			aria-label={m.common_loading()}
		/>
	{/if}
	{@render trailing?.()}
</div>
