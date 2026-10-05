<script lang="ts">
	/**
	 * 元に戻せない操作 (削除・アカウントから外す・バックアップから戻す) の確認ダイアログ。
	 * 画面ごとに違うのは見出しと説明 (と確定ボタンの文言) だけなので、枠とボタンをここに持つ。
	 * 閉じたとき (キャンセル・外側のクリック・Esc) の後始末は、呼び出し側が `open` の setter で行う。
	 */
	import type { Snippet } from 'svelte';
	import * as m from '$lib/paraglide/messages.js';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import LoadingLabel from '$lib/components/loading-label.svelte';

	type Props = {
		open: boolean;
		title: string;
		/** 確定した操作の要求中か。ボタンを押せなくし、確定ボタンに読み込み中を出す。 */
		busy: boolean;
		onconfirm: () => void;
		/** 確定ボタンの文言。既定は「削除する」。 */
		confirmLabel?: string;
		/** 説明文。 */
		children: Snippet;
	};

	let {
		open = $bindable(),
		title,
		busy,
		onconfirm,
		confirmLabel = m.action_delete_confirm(),
		children
	}: Props = $props();
</script>

<AlertDialog.Root bind:open>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{title}</AlertDialog.Title>
			<AlertDialog.Description>
				{@render children()}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={busy}>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action
				variant="destructive"
				class="relative"
				disabled={busy}
				onclick={onconfirm}
			>
				<LoadingLabel loading={busy}>{confirmLabel}</LoadingLabel>
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
