import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { components } from '$lib/api/schema';

const put = vi.fn();
vi.mock('$lib/api/client', () => ({ client: { PUT: (...args: unknown[]) => put(...args) } }));

const { ContentFormState } = await import('./content-form.svelte');

type Content = components['schemas']['AdminContentResponse'];

function savedLink(overrides: Partial<Content> = {}): Content {
	return {
		id: 1,
		type: 'link',
		title: '保存済みの題',
		url: 'https://example.com/saved',
		visibility: 'public',
		parentId: null,
		aiLinks: false,
		autoRescan: 'manual',
		listView: false,
		newFilesPublic: true,
		rootDeleted: false,
		...overrides
	};
}

describe('ContentFormState の区画ごとの保存', () => {
	beforeEach(() => put.mockReset());

	it('基本情報を保存するときは、種類ごとの区画の書きかけを送らない', async () => {
		const form = new ContentFormState();
		const saved = savedLink();
		form.loadFrom(saved, false);
		form.title = '新しい題';
		form.url = 'https://example.com/draft';

		await form.replacePart(saved.id, 'common', saved);

		const body = put.mock.calls[0][1].body;
		expect(body.title).toBe('新しい題');
		expect(body.url).toBe('https://example.com/saved');
	});

	it('種類ごとの区画を保存するときは、基本情報の書きかけを送らない', async () => {
		const form = new ContentFormState();
		const saved = savedLink();
		form.loadFrom(saved, false);
		form.title = '新しい題';
		form.url = 'https://example.com/draft';

		await form.replacePart(saved.id, 'specific', saved);

		const body = put.mock.calls[0][1].body;
		expect(body.title).toBe('保存済みの題');
		expect(body.url).toBe('https://example.com/draft');
	});

	it('保存した区画だけを読み直し、もう一方の書きかけと変更の印を残す', () => {
		const form = new ContentFormState();
		form.loadFrom(savedLink(), false);
		form.title = '新しい題';
		form.url = 'https://example.com/draft';

		form.loadPart(savedLink({ title: '新しい題' }), false, 'common');

		expect(form.partDirty('common')).toBe(false);
		expect(form.url).toBe('https://example.com/draft');
		expect(form.partDirty('specific')).toBe(true);
		expect(form.dirty).toBe(true);
	});
});
