<script lang="ts">
	import * as m from '$lib/paraglide/messages.js';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';

	type Props = {
		open: boolean;
		message: string;
		/** 省略時は汎用の失敗タイトル。画面固有の文言(loginの認証失敗等)がある場合に渡す。 */
		title?: string;
	};

	let { open = $bindable(), message, title }: Props = $props();
</script>

<AlertDialog.Root bind:open>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{title ?? m.common_error_title()}</AlertDialog.Title>
			<AlertDialog.Description>{message}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Action onclick={() => (open = false)}>{m.action_close()}</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
