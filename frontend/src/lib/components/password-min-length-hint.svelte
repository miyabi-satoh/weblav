<script lang="ts">
	import { onMount } from 'svelte';
	import { client } from '#lib/api/client.ts';
	import * as m from '#lib/paraglide/messages.js';
	import * as Field from '#lib/components/ui/field/index.ts';

	type Props = {
		/** 決まっている下限 (セットアップ)。省くと、サイトの設定の下限をサーバーに聞く。 */
		minLength?: number;
	};

	let { minLength }: Props = $props();

	let fetched = $state<number | null>(null);
	let shown = $derived(minLength ?? fetched);

	// 下限はサーバーが確かめるので、聞けなければ案内を出さないだけにする (保存はそのまま試せる)。
	onMount(async () => {
		if (minLength !== undefined) return;
		try {
			const { data } = await client.GET('/api/v1/password-policy');
			if (data) fetched = data.passwordMinLength;
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
