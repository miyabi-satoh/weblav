<script lang="ts">
	/**
	 * 見出しの段の「追加」(→ docs/ui.md「共有の中の画面」)。足せる場所でだけ置き、
	 * 「アップロード...」「新しいフォルダー...」を選ばせる。
	 */
	import PlusIcon from '@lucide/svelte/icons/plus';
	import * as m from '$lib/paraglide/messages.js';
	import type { ShareOpsState } from '$lib/share-ops-state.svelte';
	import { buttonVariants } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';

	let { ops }: { ops: ShareOpsState } = $props();
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class={[buttonVariants({ variant: 'outline' }), 'shrink-0']}>
		<PlusIcon data-icon="inline-start" />
		{m.shares_add_button()}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		<DropdownMenu.Item onSelect={() => ops.open({ kind: 'upload' })}>
			{m.shares_upload_menu()}
		</DropdownMenu.Item>
		<DropdownMenu.Item onSelect={() => ops.open({ kind: 'newFolder' })}>
			{m.shares_new_folder_menu()}
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>
