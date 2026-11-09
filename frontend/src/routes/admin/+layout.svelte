<script lang="ts">
	import FolderLockIcon from '@lucide/svelte/icons/folder-lock';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import LayoutListIcon from '@lucide/svelte/icons/layout-list';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import UsersIcon from '@lucide/svelte/icons/users';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { adminNavTabListClass, navTabClass } from '$lib/nav-tabs';
	import * as m from '$lib/paraglide/messages.js';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import RecoveryCodeDialog from '$lib/components/recovery-code-dialog.svelte';
	import WarningBand from '$lib/components/warning-band.svelte';

	let { children } = $props();

	// リカバリコードをまだ作っていない管理者には、作るよう促す (→ docs/access.md「リカバリコード」)。
	// `/admin` の下は admin にしか開けないので (→ `+layout.ts`)、ここでは role を見ない。
	let recoveryCodeMissing = $derived(page.data.user?.hasRecoveryCode === false);
	let createdCode = $state<string | null>(null);
	let creatingCode = $state(false);
	const errorDialog = new ErrorDialogState();

	// まだ無いので、作り直しの確かめは要らない。すぐ作って見せる。
	async function createRecoveryCode() {
		if (creatingCode) return;
		creatingCode = true;
		const result = await errorDialog.attempt(() => client.POST('/api/v1/auth/recovery-code'));
		creatingCode = false;
		if (result?.data) createdCode = result.data.recoveryCode;
	}

	let tabs = $derived([
		{ href: resolve('/admin/contents'), label: m.admin_contents_nav_title, icon: LayoutListIcon },
		{ href: resolve('/admin/users'), label: m.admin_users_nav_title, icon: UsersIcon },
		{ href: resolve('/admin/roots'), label: m.admin_roots_nav_title, icon: FolderLockIcon },
		{
			href: resolve('/admin/operations'),
			label: m.admin_operations_nav_title,
			icon: HistoryIcon
		},
		{ href: resolve('/admin/settings'), label: m.admin_settings_nav_title, icon: SettingsIcon }
	]);
</script>

<nav aria-label={m.admin_nav_label()} class={adminNavTabListClass}>
	{#each tabs as tab (tab.href)}
		{@const current = page.url.pathname.startsWith(tab.href)}
		<!-- スマートフォン幅では、選んでいないタブを絵だけにする。名前は読み上げ用に残す (→ docs/ui.md「UI 全般」)。 -->
		<a
			href={tab.href}
			aria-current={current ? 'page' : undefined}
			class={[navTabClass(current), 'min-w-11 justify-center gap-1.5 whitespace-nowrap']}
		>
			<tab.icon class="size-4 shrink-0" aria-hidden="true" />
			<span class={current ? undefined : 'max-sm:sr-only'}>{tab.label()}</span>
		</a>
	{/each}
</nav>
{#if recoveryCodeMissing && page.data.user}
	<WarningBand class="md:px-6">
		<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
			<p class="text-sm leading-relaxed">{m.recovery_code_missing_notice()}</p>
			<LoadingButton variant="outline" size="sm" loading={creatingCode} onclick={createRecoveryCode}
				>{m.recovery_code_missing_action()}</LoadingButton
			>
		</div>
	</WarningBand>
{/if}
{#if page.data.user}
	<!-- 見せている間に帯が消えても閉じないよう、帯の外に置く。 -->
	<RecoveryCodeDialog bind:code={createdCode} username={page.data.user.username} />
{/if}
<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
{@render children()}
