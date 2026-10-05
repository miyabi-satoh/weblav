<script lang="ts">
	import { adminPageClass } from '$lib/page-layout';
	import ArchiveItemList from '$lib/components/archive-item-list.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<!-- 見出し (タイトル・パス) とタブは [id]/+layout.svelte が出す。 -->
<div class={adminPageClass}>
	<ArchiveItemList contentId={data.contentId} editable={data.editable} />
</div>
