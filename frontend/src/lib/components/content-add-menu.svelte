<script lang="ts">
	/**
	 * 閲覧の画面 (ホーム・グループ) の見出しの段の「追加」(→ docs/ui.md「UI 全般」)。
	 * 共有の画面の「追加」と同じ形で種別を選ばせ、管理画面と同じ追加の窓を開く。
	 */
	import { tick } from 'svelte';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { fetchSharedFolderPresence } from '$lib/shared-folders';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { buttonVariants } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import ContentCreateDialog from '$lib/components/content-create-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';

	type Content = components['schemas']['AdminContentResponse'];
	type ContentType = components['schemas']['ContentType'];

	type Props = {
		/** 中に足すグループ。ホームなら `null` (ルート直下)。 */
		parentId: number | null;
	};

	let { parentId }: Props = $props();

	// 窓が親の公開範囲を重ねた見え方を出すのに、管理画面の一覧を使う。閲覧の画面の応答には祖先の公開範囲が無いため、
	// 押したときにだけ取る。
	let contents = $state<Content[]>([]);
	let hasSharedFolders = $state(false);
	let loading = $state(false);
	let createDialog = $state<ReturnType<typeof ContentCreateDialog>>();
	const errorDialog = new ErrorDialogState();

	async function openFor(type: ContentType) {
		if (loading) return;
		loading = true;
		try {
			const [list, shared] = await Promise.all([
				client.GET('/api/v1/admin/contents'),
				fetchSharedFolderPresence()
			]);
			if (!list.response.ok || !list.data) {
				errorDialog.show(errorMessage(list.error));
				return;
			}
			contents = list.data;
			hasSharedFolders = shared;
			const parent =
				parentId === null ? null : (list.data.find((content) => content.id === parentId) ?? null);
			// 窓が新しい一覧と共有の有無を受け取ってから開く。
			await tick();
			createDialog?.start(parent, type);
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			loading = false;
		}
	}
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class={[buttonVariants({ variant: 'outline' }), 'shrink-0']}>
		<PlusIcon data-icon="inline-start" />
		{m.action_add_menu()}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		<DropdownMenu.Item onSelect={() => openFor('link')}>
			{m.contents_add_link_menu()}
		</DropdownMenu.Item>
		<DropdownMenu.Item onSelect={() => openFor('folder')}>
			{m.contents_add_shortcut_menu()}
		</DropdownMenu.Item>
		<DropdownMenu.Item onSelect={() => openFor('group')}>
			{m.contents_add_group_menu()}
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>

<ContentCreateDialog bind:this={createDialog} {contents} {hasSharedFolders} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
