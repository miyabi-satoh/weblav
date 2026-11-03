<script lang="ts">
	import type { Snippet } from 'svelte';
	import FileIcon from '@lucide/svelte/icons/file';
	import { browseRowClass, browseRowPressClass, browseRowTextClass } from '$lib/list-row';
	import ListRowNewTab from '$lib/components/list-row-new-tab.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import * as m from '$lib/paraglide/messages.js';

	// 閲覧側の一覧の、ページ内では開かず新しいタブで開くファイルの行・タイル (→ $lib/list-row.ts)。
	let {
		href,
		thumbnail,
		compact = false,
		children,
		subtitle,
		leading,
		trailing
	}: {
		/** API への直リンク (→ $lib/api/urls.ts)。 */
		href: string;
		/** 縮小画像を出すなら、その URL (→ docs/ui.md「画像のプレビュー」)。 */
		thumbnail?: { src: string; original: string };
		/** フォルダー一覧の詰めた行 (`browseRowClass(true)`) にする。タイルには効かない。 */
		compact?: boolean;
		/** 行のタイトル。これを包むリンクの押せる範囲が、行いっぱいに広がる。 */
		children: Snippet;
		/** タイトルの下に置く2段目。 */
		subtitle?: Snippet;
		/** 行の文字より前に置くもの (管理画面の選択のチェックなど)。行いっぱいのリンクの上に重ねて置く。 */
		leading?: Snippet;
		/** 行の右端に置くもの (「作成者のみ」の印・⋮ のメニュー)。 */
		trailing?: Snippet;
	} = $props();
</script>

<div class={browseRowClass(compact)}>
	{@render leading?.()}
	<ListRowIcon icon={FileIcon} {compact} {thumbnail} />
	<span class={browseRowTextClass()}>
		<a {href} class={browseRowPressClass()} target="_blank" rel="external noopener noreferrer">
			<ListRowNewTab>{@render children()}</ListRowNewTab>
			<span class="sr-only">{m.contents_opens_in_new_tab()}</span>
		</a>
		{@render subtitle?.()}
	</span>
	{@render trailing?.()}
</div>
