<script lang="ts">
	import { toast } from 'svelte-sonner';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import PrinterIcon from '@lucide/svelte/icons/printer';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import { copyText } from '$lib/clipboard';
	import { CopiedState } from '$lib/copied-state.svelte';
	import { escapeHtml } from '$lib/html';
	import { formatDateTime } from '$lib/format';

	/**
	 * 作ったリカバリコードを1回だけ見せる (→ docs/access.md「リカバリコード」)。
	 * コピー・保存・印刷のどれかをするまで「保管しました」を押させない
	 * (GitHub の復旧コードの画面に倣う)。
	 */
	let { code, username, onconfirm }: { code: string; username: string; onconfirm: () => void } =
		$props();

	let kept = $state(false);
	const copyFeedback = new CopiedState();

	async function copy() {
		try {
			await copyText(code);
			kept = true;
			copyFeedback.show();
		} catch {
			toast.error(m.common_copy_failed_toast());
		}
	}

	/** 保存と印刷で同じ文面を使う。コードだけの紙やファイルでは、何のコードか分からないため。 */
	function lines(): string[] {
		return [
			m.recovery_code_file_heading(),
			'',
			`${m.recovery_code_file_username()}: ${username}`,
			`${m.recovery_code_label()}: ${code}`,
			`${m.recovery_code_file_created()}: ${formatDateTime(Date.now())}`,
			'',
			m.recovery_code_file_usage()
		];
	}

	function save() {
		const blob = new Blob([lines().join('\n') + '\n'], { type: 'text/plain;charset=utf-8' });
		const href = URL.createObjectURL(blob);
		const link = document.createElement('a');
		link.href = href;
		link.download = `weblav-recovery-code-${username}.txt`;
		link.click();
		// すぐに無効にすると、Safari ではダウンロードが始まる前に URL が消えて保存されない。
		setTimeout(() => URL.revokeObjectURL(href), 1000);
		kept = true;
	}

	let printFrame: HTMLIFrameElement | undefined;

	/**
	 * 画面ごと印刷すると余計なものまで出るので、文面だけの見えない iframe を印刷する。
	 * 印刷の窓が閉じるまで「保管しました」を押させない。
	 */
	function print() {
		printFrame?.remove();
		const iframe = document.createElement('iframe');
		printFrame = iframe;
		iframe.style.position = 'fixed';
		iframe.style.width = '0';
		iframe.style.height = '0';
		iframe.style.border = '0';
		iframe.srcdoc = `<!doctype html><meta charset="utf-8"><title>WebLAV</title><body style="font-family: sans-serif; line-height: 1.8">${lines()
			.map((line) => `<div>${escapeHtml(line) || '&nbsp;'}</div>`)
			.join('')}</body>`;
		iframe.onload = () => {
			const win = iframe.contentWindow;
			if (!win) return;
			win.addEventListener('afterprint', () => {
				iframe.remove();
				kept = true;
			});
			win.print();
		};
		document.body.appendChild(iframe);
	}
</script>

<div class="flex min-w-0 flex-col gap-6">
	<p class="text-sm leading-relaxed text-muted-foreground">{m.recovery_code_description()}</p>
	<div class="flex flex-col gap-2">
		<span class="text-sm font-medium">{m.recovery_code_label()}</span>
		<!-- 書体は本文と同じ (→ docs/ui.md「UI 全般」)。UD 書体なので、紛らわしい字は見分けられる。
		     コードは1行で見せる。狭い枠では字の大きさを段階で下げて収める (23字で 17em ほど)。 -->
		<div class="@container">
			<output
				class="block rounded border bg-muted px-3 py-2 text-center text-xs font-bold whitespace-nowrap select-all @[17rem]:text-sm @[19rem]:text-base @[21rem]:text-lg"
				>{code}</output
			>
		</div>
	</div>
	<div class="flex flex-wrap gap-2">
		<Button variant="outline" onclick={copy}>
			{#if copyFeedback.copied}
				<CheckIcon data-icon="inline-start" />
			{:else}
				<CopyIcon data-icon="inline-start" />
			{/if}{m.recovery_code_copy()}
		</Button>
		<Button variant="outline" onclick={save}>
			<DownloadIcon data-icon="inline-start" />{m.recovery_code_save()}
		</Button>
		<Button variant="outline" onclick={print}>
			<PrinterIcon data-icon="inline-start" />{m.recovery_code_print()}
		</Button>
	</div>
	<div class="flex flex-col gap-2">
		<Button class="w-full" disabled={!kept} onclick={onconfirm}>{m.recovery_code_confirm()}</Button>
		{#if !kept}
			<p class="text-sm text-muted-foreground">{m.recovery_code_confirm_hint()}</p>
		{/if}
	</div>
</div>
