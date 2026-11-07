import { describe, expect, it } from 'vitest';
import { pickShareUrl, shareableUrls } from './share-url';

const connection = {
	mdnsHostname: 'teacher-pc.local',
	port: 8080,
	lanAddresses: ['192.168.1.13']
};

describe('shareableUrls', () => {
	it('ほかの端末から開いているアドレスを先頭に、名前と IP のアドレスを続ける', () => {
		expect(
			shareableUrls(new URL('http://teacher-pc.local:8080/folders/3?path=a#x'), connection)
		).toEqual([
			'http://teacher-pc.local:8080/folders/3?path=a',
			'http://192.168.1.13:8080/folders/3?path=a'
		]);
	});

	it('HTTPS の前段越しに開いているときは、そのアドレスだけ', () => {
		expect(shareableUrls(new URL('https://weblav.school.example/groups/2'), connection)).toEqual([
			'https://weblav.school.example/groups/2'
		]);
	});

	it('PC の前で開いているアドレスは、PC の名前と IP に置き換える', () => {
		for (const origin of ['http://localhost:5173', 'http://127.0.0.1:8080', 'http://[::1]:8080']) {
			expect(shareableUrls(new URL(`${origin}/archives/1?item=2`), connection), origin).toEqual([
				'http://teacher-pc.local:8080/archives/1?item=2',
				'http://192.168.1.13:8080/archives/1?item=2'
			]);
		}
	});

	it('PC の名前を読めなくても、IP で渡す', () => {
		expect(
			shareableUrls(new URL('http://localhost:8080/'), { ...connection, mdnsHostname: null })
		).toEqual(['http://192.168.1.13:8080/']);
	});

	it('ほかの端末から開けないときは、PC の前のアドレスを渡さない', () => {
		const local = new URL('http://localhost:8080/');
		expect(shareableUrls(local, undefined)).toEqual([]);
		expect(shareableUrls(local, { mdnsHostname: null, port: 8080, lanAddresses: [] })).toEqual([]);
		expect(shareableUrls(local, { ...connection, port: 0 })).toEqual([]);
	});
});

describe('pickShareUrl', () => {
	const urls = ['http://teacher-pc.local:8080/', 'http://192.168.1.13:8080/'];

	it('前に選んだホストのアドレスを選ぶ', () => {
		expect(pickShareUrl(urls, '192.168.1.13:8080')).toBe('http://192.168.1.13:8080/');
	});

	it('前に選んだホストが無くなっていれば、先頭を選ぶ', () => {
		expect(pickShareUrl(urls, '192.168.1.99:8080')).toBe('http://teacher-pc.local:8080/');
		expect(pickShareUrl(urls, null)).toBe('http://teacher-pc.local:8080/');
		expect(pickShareUrl([], null)).toBeUndefined();
	});
});
