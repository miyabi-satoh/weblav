import { describe, expect, it } from 'vitest';
import * as m from '#lib/paraglide/messages.js';
import { uploadItemNote, type UploadItem } from './share-upload-dialog.svelte';

const file = new File(['x'], 'IMG_0001.heic');

function item(patch: Partial<UploadItem>): UploadItem {
	return { file, state: 'waiting', progress: 0, ...patch };
}

describe('uploadItemNote', () => {
	it('names the file it was saved as when the server renamed it', () => {
		expect(
			uploadItemNote(
				item({
					state: 'done',
					result: { name: 'a (1).jpg', renamed: true, extensionFixed: false }
				}),
				null
			)
		).toBe(m.shares_upload_renamed({ name: 'a (1).jpg' }));
		expect(
			uploadItemNote(
				item({ state: 'done', result: { name: 'a.jpg', renamed: false, extensionFixed: true } }),
				null
			)
		).toBe(m.shares_upload_extension_fixed({ name: 'a.jpg' }));
	});

	it('says nothing for a file saved under its own name', () => {
		expect(
			uploadItemNote(
				item({ state: 'done', result: { name: 'a.jpg', renamed: false, extensionFixed: false } }),
				null
			)
		).toBeUndefined();
	});

	it('explains failures and files over the limit', () => {
		expect(uploadItemNote(item({ state: 'failed', message: 'nope' }), null)).toBe('nope');
		expect(uploadItemNote(item({ state: 'tooLarge' }), 1024)).toBe(
			m.shares_upload_too_large({ size: '1.0\u00a0KB' })
		);
	});
});
