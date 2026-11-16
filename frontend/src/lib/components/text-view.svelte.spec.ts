import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import TextView from './text-view.svelte';
import { MAX_FORMATTED_LENGTH } from '#lib/code-highlight.ts';

function textUrl(text: string): string {
	return URL.createObjectURL(new Blob([text], { type: 'text/markdown' }));
}

describe('TextView', () => {
	it('tells the viewer when a Markdown file is too large to preview, so the toggle shows the source', async () => {
		const onpreviewunavailable = vi.fn();
		await render(TextView, {
			src: textUrl('a'.repeat(MAX_FORMATTED_LENGTH + 1)),
			fileName: 'large.md',
			onpreviewunavailable,
			onerror: () => {}
		});
		await expect.poll(() => onpreviewunavailable.mock.calls.length).toBe(1);
	});

	it('does not for a Markdown file it can preview', async () => {
		const onpreviewunavailable = vi.fn();
		await render(TextView, {
			src: textUrl('# 見出し'),
			fileName: 'small.md',
			onpreviewunavailable,
			onerror: () => {}
		});
		await expect.poll(() => document.querySelector('h1')?.textContent).toBe('見出し');
		expect(onpreviewunavailable).not.toHaveBeenCalled();
	});
});
