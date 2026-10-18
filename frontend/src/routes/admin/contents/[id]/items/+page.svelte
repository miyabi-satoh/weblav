<script lang="ts">
	import { adminPageClass } from '$lib/page-layout';
	import { invalidateAll } from '$app/navigation';
	import ArchiveItemList from '$lib/components/archive-item-list.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<!-- 見出し (タイトル・パス) とタブは [id]/+layout.svelte が出す。 -->
<div class={adminPageClass}>
	<!-- 見出しの件数は layout が一覧の API から出すので、変えたら取り直す。 -->
	<ArchiveItemList
		contentId={data.contentId}
		editable={data.editable}
		onchanged={() => void invalidateAll()}
	/>
</div>
