<script lang="ts">
	import type { ClassValue } from 'svelte/elements';

	// 表に出すパス。収まらないときはフォルダーの側を「…」で切り、末尾のファイル名を残す (Finder の中省略と同じ形)。
	// 同じタイトルが並ぶアイテムの一覧では、見分けに要るのはファイル名のほうのため。
	// ファイル名だけでも収まらなければ、ファイル名も「…」で切る。
	// 先頭に「…」を出す `direction: rtl` の形は、WebKit で「…」と文字のあいだが空くので使わない。
	let { path, class: className }: { path: string; class?: ClassValue } = $props();

	let slash = $derived(path.lastIndexOf('/'));
</script>

<!-- flex で2つに分けると、読み上げや行の検索で「2024/ answer.pdf」と区切れる。1続きのパスは sr-only で持たせる。 -->
<span class={['flex min-w-0', className]} title={path}>
	<span class="sr-only">{path}</span>
	<span class="min-w-0 truncate" aria-hidden="true">{path.slice(0, slash + 1)}</span>
	<span class="max-w-full shrink-0 truncate" aria-hidden="true">{path.slice(slash + 1)}</span>
</span>
