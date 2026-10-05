<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Spinner } from '$lib/components/ui/spinner';
	import * as m from '$lib/paraglide/messages.js';

	let { loading, children }: { loading: boolean; children: Snippet } = $props();
</script>

<!-- 親のボタンに `relative` を付けて使う。loading中はラベルを非表示(invisible)にしてスペースだけ
     保持し、スピナーを絶対配置で中央に重ねる。ボタンの幅がラベル→スピナーの切り替えで変わらないようにするため。
     `LoadingButton` を使えない確定ボタン (確認ダイアログの `AlertDialog.Action`) でも同じ見た目にする。 -->
{#if loading}
	<span class="absolute inset-0 flex items-center justify-center">
		<Spinner aria-label={m.common_loading()} />
	</span>
{/if}
<!-- contents にして、アイコンと文言を親のボタンの flex (gap) に直接並べる。
     普通の span で包むと、アイコンが文言の上に積まれる。 -->
<span class={['contents', loading && 'invisible']}>
	{@render children()}
</span>
