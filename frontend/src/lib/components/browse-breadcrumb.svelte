<script lang="ts">
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import { breadcrumbLinkClass, breadcrumbNavClass } from '$lib/breadcrumb';
	import type { components } from '$lib/api/schema';
	import type { Snippet } from 'svelte';

	type Props = {
		/** ルートに近い順の祖先グループ。APIの `ancestors` をそのまま渡す。 */
		ancestors: components['schemas']['GroupAncestor'][];
		/** 祖先の後ろに続く部分 (現在地やフォルダー内のサブパス)。区切りも含めて書く。 */
		children: Snippet;
	};

	let { ancestors, children }: Props = $props();
</script>

<!-- 閲覧側のパンくずの先頭 (ホーム → 祖先グループ)。グループ・フォルダー・アーカイブの
     どの画面からもトップと親グループへ戻れるようにするため、3画面で共有する。 -->
<nav class={breadcrumbNavClass}>
	<a href={resolve('/')} class={breadcrumbLinkClass}>{m.breadcrumb_home()}</a>
	{#each ancestors as ancestor (ancestor.id)}
		<span aria-hidden="true">/</span>
		<a href={resolve('/groups/[id]', { id: String(ancestor.id) })} class={breadcrumbLinkClass}>
			{ancestor.title}
		</a>
	{/each}
	{@render children()}
</nav>
