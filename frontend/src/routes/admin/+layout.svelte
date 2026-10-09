<script lang="ts">
	import FolderLockIcon from '@lucide/svelte/icons/folder-lock';
	import LayoutListIcon from '@lucide/svelte/icons/layout-list';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import UsersIcon from '@lucide/svelte/icons/users';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { isAdmin } from '$lib/auth';
	import { adminNavTabListClass, navTabClass } from '$lib/nav-tabs';
	import * as m from '$lib/paraglide/messages.js';
	import { canManageRoots } from '$lib/root-location';
	import { Button } from '$lib/components/ui/button';
	import RecoveryCodeDialog from '$lib/components/recovery-code-dialog.svelte';
	import WarningBand from '$lib/components/warning-band.svelte';

	let { children } = $props();

	// タブの列は admin にだけ出す (→ docs/ui.md「UI 全般」)。
	let currentUserIsAdmin = $derived(isAdmin(page.data.user));
	// リカバリコードをまだ作っていない管理者には、作るよう促す (→ docs/access.md「リカバリコード」)。
	let recoveryCodeMissing = $derived(
		currentUserIsAdmin && page.data.user?.hasRecoveryCode === false
	);
	let recoveryCodeOpen = $state(false);

	// 「公開できるフォルダー」はサーバーの PC からしか触れない (→ docs/folders.md「公開できるフォルダー」)。
	// LAN の端末では API が 404 を返すので、入り口も出さない。
	let tabs = $derived([
		{ href: resolve('/admin/contents'), label: m.admin_contents_nav_title, icon: LayoutListIcon },
		{ href: resolve('/admin/users'), label: m.admin_users_nav_title, icon: UsersIcon },
		...(canManageRoots(page.data.user, page.url.hostname)
			? [{ href: resolve('/admin/roots'), label: m.admin_roots_nav_title, icon: FolderLockIcon }]
			: []),
		{ href: resolve('/admin/settings'), label: m.admin_settings_nav_title, icon: SettingsIcon }
	]);
</script>

{#if currentUserIsAdmin}
	<nav aria-label={m.admin_nav_label()} class={adminNavTabListClass}>
		{#each tabs as tab (tab.href)}
			<!-- コンテンツの編集ページ (/admin/contents/[id]/...) もコンテンツのタブを選択中にする。 -->
			{@const current = page.url.pathname.startsWith(tab.href)}
			<!-- スマートフォン幅では、選んでいないタブを絵だけにする。名前は読み上げ用に残す (→ docs/ui.md「UI 全般」)。 -->
			<a
				href={tab.href}
				aria-current={current ? 'page' : undefined}
				class={[navTabClass(current), 'min-w-11 justify-center gap-1.5 whitespace-nowrap']}
			>
				<tab.icon class="size-4 shrink-0 sm:hidden" aria-hidden="true" />
				<span class={current ? undefined : 'max-sm:sr-only'}>{tab.label()}</span>
			</a>
		{/each}
	</nav>
{/if}
{#if recoveryCodeMissing && page.data.user}
	<WarningBand class="md:px-6">
		<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
			<p class="text-sm leading-relaxed">{m.recovery_code_missing_notice()}</p>
			<Button variant="outline" size="sm" onclick={() => (recoveryCodeOpen = true)}
				>{m.recovery_code_missing_action()}</Button
			>
		</div>
	</WarningBand>
	<RecoveryCodeDialog bind:open={recoveryCodeOpen} username={page.data.user.username} />
{/if}
{@render children()}
