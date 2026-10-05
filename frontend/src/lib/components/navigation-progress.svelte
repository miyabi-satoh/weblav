<script lang="ts">
	import { navigating } from '$app/state';
	import * as m from '$lib/paraglide/messages.js';

	// 画面を移る途中 (移る先の load を待つ間) に、画面の上端へ細いバーを出す
	// (→ docs/ui.md「UI 全般」)。すぐ終わる移動でちらつかないよう、少し待ってから出す。
	const SHOW_DELAY_MS = 200;

	let visible = $state(false);

	$effect(() => {
		if (navigating.to === null) {
			visible = false;
			return;
		}
		const timer = setTimeout(() => (visible = true), SHOW_DELAY_MS);
		return () => clearTimeout(timer);
	});
</script>

{#if visible}
	<!-- 進み具合は分からないので値を持たない (不定の progressbar)。 -->
	<div
		role="progressbar"
		aria-label={m.common_loading()}
		class="pointer-events-none fixed inset-x-0 top-0 z-50 h-1 overflow-hidden"
	>
		<div class="h-full navigation-progress-bar bg-primary"></div>
	</div>
{/if}
