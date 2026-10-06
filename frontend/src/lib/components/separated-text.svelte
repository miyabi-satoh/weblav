<script lang="ts">
	// 2段目に並べる値を「·」で区切る。区切りは見た目だけのもので、読み上げさせない。
	let { text }: { text: string | string[] } = $props();

	let parts = $derived(typeof text === 'string' ? [text] : text);

	// 区切りの前後の空白は式で出す。ブロックの内側の端に書いた空白は、Svelte が文字の参照でも落とすため。
	const SPACE = ' ';
</script>

{#each parts as part, index (index)}{#if index > 0}{SPACE}<span aria-hidden="true">·</span
		>{SPACE}{/if}{part}{/each}
