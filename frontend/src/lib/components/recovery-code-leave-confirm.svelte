<script lang="ts">
	/**
	 * 作ったばかりのリカバリコードを見せている画面 (初回セットアップ・パスワードの再設定) で、
	 * 「保管しました」を押す前に離れるときの確認。コードは二度と見せられず、サーバーはもう
	 * 「コードがある」と覚えている (作るよう促す帯も出ない) ため。
	 */
	import LeaveConfirmDialog from '#lib/components/leave-confirm-dialog.svelte';
	import { LeaveGuard } from '#lib/leave-guard.svelte.ts';
	import * as m from '#lib/paraglide/messages.js';

	let {
		active,
		ondiscard
	}: {
		/** コードを見せていて、まだ「保管しました」を押していないか。 */
		active: boolean;
		/** 確認で離れるのを選んだとき。移った先で確認をもう一度出さないよう、`active` を下ろす。 */
		ondiscard: () => void;
	} = $props();

	const guard = new LeaveGuard(() => active);
</script>

<LeaveConfirmDialog
	{guard}
	{ondiscard}
	title={m.recovery_code_leave_title()}
	description={m.recovery_code_leave_description()}
	action={m.recovery_code_leave_button()}
/>
