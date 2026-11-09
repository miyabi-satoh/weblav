<script lang="ts">
	import CircleUserRoundIcon from '@lucide/svelte/icons/circle-user-round';
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { headerTextActionClass } from '$lib/header-action';
	import { cn } from '$lib/utils';
	import { formatNumber } from '$lib/format';

	let {
		username,
		hiddenCount,
		isAdmin,
		onlogout
	}: {
		username: string;
		/** 管理画面の入口を出すか。管理画面は admin 専用 (→ docs/access.md「ロールと操作」)。 */
		isAdmin: boolean;
		/** 非表示のコンテンツの一覧に並ぶ件数。0 なら項目を出さない。 */
		hiddenCount: number;
		onlogout: () => void;
	} = $props();
</script>

<DropdownMenu.Root>
	<!-- 狭い幅でヘッダーからはみ出さないよう、Button の shrink-0 を打ち消し、残りの幅で名前を省略する。 -->
	<DropdownMenu.Trigger class={cn(headerTextActionClass, 'min-w-0 shrink')}>
		<!-- 隣の言語・テーマのアイコンと大きさを揃える (sm の既定は size-3.5)。 -->
		<CircleUserRoundIcon data-icon="inline-start" class="size-5" />
		<span class="max-w-40 min-w-0 truncate">{username}</span>
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		<DropdownMenu.Group>
			<DropdownMenu.Label class="wrap-anywhere">{username}</DropdownMenu.Label>
			<DropdownMenu.Separator />
			{#if isAdmin}
				<DropdownMenu.Item>
					{#snippet child({ props })}
						<a href={resolve('/admin/contents')} {...props}>{m.admin_nav_label()}</a>
					{/snippet}
				</DropdownMenu.Item>
			{/if}
			{#if hiddenCount > 0}
				<!-- 非表示のものは閲覧の場のどこにも並ばないので、戻す入口をここに置く。 -->
				<DropdownMenu.Item>
					{#snippet child({ props })}
						<a href={resolve('/hidden')} {...props}
							>{m.hidden_menu_item({ count: formatNumber(hiddenCount) })}</a
						>
					{/snippet}
				</DropdownMenu.Item>
			{/if}
			<!-- ログイン後にマニュアルを引き直す導線がここにしかない (ログイン画面・
			     フォルダー選択・軸の画面のリンクは、いずれも使っている最中にしか出ない)。 -->
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a href={resolve('/help')} {...props}>{m.help_eyebrow()}</a>
				{/snippet}
			</DropdownMenu.Item>
			<!-- ユーザー名・パスワード・リカバリコードは、アカウントのページにまとめる (→ docs/ui.md「アカウントのページ」)。 -->
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a href={resolve('/account')} {...props}>{m.account_title()}</a>
				{/snippet}
			</DropdownMenu.Item>
			<DropdownMenu.Item onSelect={onlogout}>{m.logout_button()}</DropdownMenu.Item>
		</DropdownMenu.Group>
	</DropdownMenu.Content>
</DropdownMenu.Root>
