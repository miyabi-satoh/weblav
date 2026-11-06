<script lang="ts">
	// 共有がつながらない・書き込めないときの知らせ。管理の一覧の行と編集ページの見出しの下で同じものを出す
	// (閲覧側では開いたときに初めて分かるので、管理の画面で先に気づけるようにする。→ docs/folders.md「つながっているかの確かめ」)。
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import type { components } from '$lib/api/schema';
	import * as m from '$lib/paraglide/messages.js';

	type RootStatus = components['schemas']['RootStatus'];

	let {
		status,
		writable,
		size
	}: {
		status: RootStatus;
		writable: boolean;
		/** 一覧の行は小さく (`xs`)、編集ページは本文の大きさ (`sm`) で出す。 */
		size: 'xs' | 'sm';
	} = $props();

	let messages = $derived(
		[
			status === 'unreachable' ? m.admin_roots_status_unreachable() : '',
			status === 'missing' ? m.admin_roots_status_missing() : '',
			writable ? '' : m.admin_roots_not_writable()
		].filter((message) => message !== '')
	);
</script>

{#each messages as message (message)}
	<span
		class={[
			'mt-1 flex items-start gap-1.5 text-warning-muted',
			size === 'xs' ? 'text-xs' : 'text-sm'
		]}
	>
		<TriangleAlertIcon
			class={['shrink-0 text-warning-icon', size === 'xs' ? 'mt-px size-3.5' : 'mt-0.5 size-4']}
		/>
		{message}
	</span>
{/each}
