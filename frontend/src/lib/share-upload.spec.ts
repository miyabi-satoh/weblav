import { describe, expect, it } from 'vitest';
import { shareUploadUrl, uploadOutcome } from './share-upload';
import { errorMessage } from './api/errors';

describe('shareUploadUrl', () => {
	it('escapes the folder path into the query', () => {
		expect(shareUploadUrl(3, '教材/a b')).toBe(
			'/api/v1/shares/3/upload?path=%E6%95%99%E6%9D%90%2Fa+b'
		);
		expect(shareUploadUrl(3, '')).toBe('/api/v1/shares/3/upload?path=');
	});
});

describe('uploadOutcome', () => {
	it('returns the placed file of a single-file upload', () => {
		const file = { name: 'a (1).jpg', renamed: true, extensionFixed: false };
		expect(uploadOutcome(201, { files: [file] })).toEqual({ ok: true, file });
	});

	it('turns an error envelope into its message', () => {
		const body = { error: { code: 'file_too_large', message: 'too large' } };
		expect(uploadOutcome(413, body)).toEqual({
			ok: false,
			message: errorMessage(body),
			aborted: false
		});
	});

	it('treats a success without the placed file as a failure', () => {
		expect(uploadOutcome(201, null)).toMatchObject({ ok: false, aborted: false });
	});
});
