<script lang="ts">
	import { resolve } from '$app/paths';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import * as m from '$lib/paraglide/messages.js';
	import { headerIconActionClass } from '$lib/header-action';
	import { cn } from '$lib/utils';
	import { pageHeadingTextClass } from '$lib/page-layout';

	// 閲覧の画面の見出し。書き換えられる人には、右端に編集ページへのペンを出す (→ docs/ui.md「UI 全般」)。

	type Props = {
		/** 見出しの文字。フォルダーの中やリンクの一覧のファイルでは、編集するコンテンツの名前と違う。 */
		heading: string;
		/** 編集ページを開くコンテンツ。 */
		contentId: number;
		/** 編集するコンテンツの名前。ペンの読み上げに使う。 */
		contentTitle: string;
		canEdit: boolean;
	};

	let { heading, contentId, contentTitle, canEdit }: Props = $props();
</script>

<div class="mb-4 flex items-start gap-2">
	<h1 class={['min-w-0 flex-1 wrap-anywhere', pageHeadingTextClass]}>{heading}</h1>
	{#if canEdit}
		<!-- 見出しの行の高さを変えないよう上下を打ち消し、アイコンの右の端を余白の線に揃える。 -->
		<a
			href={resolve('/admin/contents/[id]', { id: String(contentId) })}
			class={cn(headerIconActionClass, '-my-0.5 -mr-2 shrink-0')}
		>
			<PencilIcon />
			<span class="sr-only">{m.browse_heading_edit_label({ title: contentTitle })}</span>
		</a>
	{/if}
</div>
