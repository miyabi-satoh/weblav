<script lang="ts">
	/**
	 * 作成ダイアログの1段目。種別ごとに一行の説明を添えて並べ、押したら2段目へ進む。
	 * 種別の名前だけでは、フォルダーとアーカイブのように違いが分からないものがあるため。
	 */
	import { contentTypeDescription, contentTypeLabel } from '$lib/content-labels';
	import { contentTypeIcon } from '$lib/content-types';
	import type { components } from '$lib/api/schema';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';

	type ContentType = components['schemas']['ContentType'];

	type Props = {
		/** 並べる種別。並びは画面に出す順。 */
		types: ContentType[];
		/** 一覧の読み上げ用の名前。 */
		label: string;
		/** いまは選べない種別。行は残したまま押せなくし、理由を添える。 */
		disabledTypes?: ContentType[];
		/** 押せない行に添える短い理由。`disabledTypes` があるときだけ使う。 */
		disabledReason?: string;
		/** 一覧の下に出す、どうすれば選べるようになるかの案内。 */
		disabledNote?: string;
		onselect: (type: ContentType) => void;
	};

	let {
		types,
		label,
		disabledTypes = [],
		disabledReason = '',
		disabledNote = '',
		onselect
	}: Props = $props();

	let showNote = $derived(
		disabledNote !== '' && types.some((type) => disabledTypes.includes(type))
	);
</script>

<ul aria-label={label} class="flex flex-col divide-y overflow-hidden rounded-lg border">
	{#each types as type (type)}
		{@const TypeIcon = contentTypeIcon(type)}
		{@const disabled = disabledTypes.includes(type)}
		<li>
			<!-- 押せない行も消さずに残す。何が足りないのかを伝えるため。 -->
			<button
				type="button"
				{disabled}
				class="flex min-h-14 w-full items-center gap-3 px-3 py-2 text-left outline-none not-disabled:hover:bg-muted not-disabled:focus-visible:bg-muted disabled:bg-muted/40"
				onclick={() => onselect(type)}
			>
				<TypeIcon
					class="size-5 shrink-0 {disabled ? 'text-muted-foreground/50' : 'text-muted-foreground'}"
				/>
				<span class="flex min-w-0 flex-1 flex-col {disabled ? 'text-muted-foreground' : ''}">
					<span class="font-medium">{contentTypeLabel(type)}</span>
					<span class="text-sm text-muted-foreground">{contentTypeDescription(type)}</span>
					<!-- 行の右に置くと説明文の幅を取り、細かく折り返すので、説明文の下に置く。 -->
					{#if disabled && disabledReason !== ''}
						<span class="text-xs text-muted-foreground">{disabledReason}</span>
					{/if}
				</span>
				<ChevronRightIcon
					class="size-4 shrink-0 {disabled ? 'text-muted-foreground/40' : 'text-muted-foreground'}"
				/>
			</button>
		</li>
	{/each}
</ul>
{#if showNote}
	<p class="mt-3 text-sm text-muted-foreground">{disabledNote}</p>
{/if}
