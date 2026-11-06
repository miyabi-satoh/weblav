<script lang="ts">
	/**
	 * 編集ページの最後に置く「削除」の区画。消すと何が起きるかを書き、確認の窓を開くボタンを置く
	 * (→ docs/ui.md「編集ページの区画」)。区画の石垣の下に、1列目と同じ幅で置く。
	 */
	import { Button } from '$lib/components/ui/button';
	import { sectionCardClass, sectionGridClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';

	type Props = {
		/** 消すと何が起きるか。1つずつ段落にする。 */
		notes: string[];
		/** 押せない理由。あれば、ボタンを押せなくして書く。 */
		disabledReason?: string | null;
		onclick: () => void;
	};

	let { notes, disabledReason = null, onclick }: Props = $props();
</script>

<!-- 上の石垣 (`masonry`) は最後の区画の下にも区画の間と同じ隙間を残すので、ここでは上に間を足さない。 -->
<div class={sectionGridClass}>
	<section aria-labelledby="delete-section-heading" class={sectionCardClass}>
		<h2 id="delete-section-heading" class="text-base font-semibold">
			{m.common_delete_section_title()}
		</h2>
		<div class="flex flex-col gap-2 text-sm text-muted-foreground">
			{#each notes as note (note)}
				<p>{note}</p>
			{/each}
			{#if disabledReason}
				<p>{disabledReason}</p>
			{/if}
		</div>
		<div>
			<!-- 確定は確認の窓の赤いボタンで行うので、ここでは窓を開くだけの普通のボタンにする。 -->
			<Button variant="outline" disabled={disabledReason !== null} {onclick}>
				{m.action_delete()}
			</Button>
		</div>
	</section>
</div>
