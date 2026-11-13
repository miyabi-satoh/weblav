<script lang="ts">
	/**
	 * 編集ページの最後に置く「削除」の区画。消すと何が起きるかを書き、確認の窓を開くボタンを置く
	 * (→ docs/ui.md「編集ページの区画」)。区画の石垣の下に、1列目と同じ幅で置く。
	 */
	import { Button } from '#lib/components/ui/button/index.ts';
	import { sectionCardClass, sectionGridClass } from '#lib/page-layout.ts';
	import * as m from '#lib/paraglide/messages.js';

	type Props = {
		/** 消すと何が起きるか。1つずつ段落にする。 */
		notes: string[];
		onclick: () => void;
	};

	let { notes, onclick }: Props = $props();
</script>

<!-- 上の石垣 (`masonry`) は最後の区画の下にも区画の間と同じ隙間を残すので、ここでは上に間を足さない。 -->
<div class={sectionGridClass}>
	<!-- 消す操作だと区画を見ただけで分かるよう、枠と見出しを危険の色にする (→ docs/ui.md「UI 全般」)。 -->
	<section
		aria-labelledby="delete-section-heading"
		class={[sectionCardClass, 'border-destructive/40']}
	>
		<h2 id="delete-section-heading" class="text-base font-semibold text-destructive">
			{m.common_delete_section_title()}
		</h2>
		<div class="flex flex-col gap-2 text-sm text-muted-foreground">
			{#each notes as note (note)}
				<p>{note}</p>
			{/each}
		</div>
		<div>
			<!-- 押しても確認の窓が開くだけなので、見えるところに置いても誤って消すことはない。 -->
			<Button variant="destructive" {onclick}>
				{m.action_delete()}
			</Button>
		</div>
	</section>
</div>
