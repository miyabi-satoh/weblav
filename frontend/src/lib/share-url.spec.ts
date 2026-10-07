import { describe, expect, it } from 'vitest';
import { shareableUrl } from './share-url';

const connection = { mdnsHostname: 'teacher-pc.local', port: 8080 };

describe('shareableUrl', () => {
	it('ほかの端末から開いているアドレスは、そのまま渡す', () => {
		expect(
			shareableUrl(new URL('http://teacher-pc.local:8080/folders/3?path=a#x'), connection)
		).toBe('http://teacher-pc.local:8080/folders/3?path=a');
		expect(shareableUrl(new URL('https://weblav.school.example/groups/2'), undefined)).toBe(
			'https://weblav.school.example/groups/2'
		);
	});

	it('PC の前で開いているアドレスは、PC の名前とサーバーのポートに置き換える', () => {
		for (const origin of ['http://localhost:5173', 'http://127.0.0.1:8080', 'http://[::1]:8080']) {
			expect(shareableUrl(new URL(`${origin}/archives/1?item=2`), connection), origin).toBe(
				'http://teacher-pc.local:8080/archives/1?item=2'
			);
		}
	});

	it('PC の名前で案内できなければ、渡さない', () => {
		const local = new URL('http://localhost:8080/');
		expect(shareableUrl(local, undefined)).toBeUndefined();
		expect(shareableUrl(local, { mdnsHostname: null, port: 8080 })).toBeUndefined();
		expect(shareableUrl(local, { mdnsHostname: 'teacher-pc.local', port: 0 })).toBeUndefined();
	});
});
