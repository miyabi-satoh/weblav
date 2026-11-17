<script lang="ts">
	import { onMount } from 'svelte';
	import { client } from '#lib/api/client.ts';
	import * as m from '#lib/paraglide/messages.js';
	import * as Field from '#lib/components/ui/field/index.ts';

	type Props = {
		/** サイトの設定でなく既定の下限を出す (最初のセットアップは既定で固定するため)。 */
		useDefault?: boolean;
	};

	let { useDefault = false }: Props = $props();

	let shown = $state<number | null>(null);

	// 下限はサーバーが確かめるので、聞けなければ案内を出さないだけにする (保存はそのまま試せる)。
	onMount(async () => {
		try {
			const { data } = await client.GET('/api/v1/password-policy');
			if (data) shown = useDefault ? data.defaultPasswordMinLength : data.passwordMinLength;
		} catch {
			// 案内だけなので、出さずに済ませる。
		}
	});
</script>

{#if shown !== null}
	<Field.FieldDescription
		>{m.common_password_min_length_description({ count: shown })}</Field.FieldDescription
	>
{/if}
