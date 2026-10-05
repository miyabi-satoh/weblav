import { describe, expect, it } from 'vitest';
import { isSamePath, pathTrail } from './path-trail';

describe('pathTrail', () => {
	it('returns nothing for the root listing', () => {
		expect(pathTrail('')).toEqual([]);
	});

	it('splits a POSIX path and keeps the root separator', () => {
		expect(pathTrail('/Users/foo/materials')).toEqual([
			{ label: '/', path: '/' },
			{ label: 'Users', path: '/Users' },
			{ label: 'foo', path: '/Users/foo' },
			{ label: 'materials', path: '/Users/foo/materials' }
		]);
	});

	it('treats the root itself as a single crumb', () => {
		expect(pathTrail('/')).toEqual([{ label: '/', path: '/' }]);
	});

	// `C:` だけを切り出すとドライブ相対パスになり、`C:\` と意味が変わる。
	it('keeps the trailing separator on a Windows drive root', () => {
		expect(pathTrail('C:\\教材\\英検')).toEqual([
			{ label: 'C:\\', path: 'C:\\' },
			{ label: '教材', path: 'C:\\教材' },
			{ label: '英検', path: 'C:\\教材\\英検' }
		]);
	});

	// UNC は共有名まで揃って初めてパスとして成立するため、分割してはいけない。
	it('keeps a UNC server and share together as the root', () => {
		expect(pathTrail('\\\\server\\share\\教材')).toEqual([
			{ label: '\\\\server\\share', path: '\\\\server\\share' },
			{ label: '教材', path: '\\\\server\\share\\教材' }
		]);
	});

	it('handles a UNC share root with nothing below it', () => {
		expect(pathTrail('\\\\server\\share')).toEqual([
			{ label: '\\\\server\\share', path: '\\\\server\\share' }
		]);
	});

	// バックエンドが返すのは canonicalize 済みのパスで、Windows ではこの形になる。
	it('keeps the verbatim prefix with the drive root', () => {
		expect(pathTrail('\\\\?\\C:\\教材\\英検')).toEqual([
			{ label: '\\\\?\\C:\\', path: '\\\\?\\C:\\' },
			{ label: '教材', path: '\\\\?\\C:\\教材' },
			{ label: '英検', path: '\\\\?\\C:\\教材\\英検' }
		]);
	});

	it('keeps the verbatim UNC prefix with the server and share', () => {
		expect(pathTrail('\\\\?\\UNC\\server\\share\\教材')).toEqual([
			{ label: '\\\\?\\UNC\\server\\share', path: '\\\\?\\UNC\\server\\share' },
			{ label: '教材', path: '\\\\?\\UNC\\server\\share\\教材' }
		]);
	});

	// Unix系では `\` はディレクトリ名に使える普通の文字。区切りにしてはいけない。
	it('does not split on a backslash inside a POSIX directory name', () => {
		expect(pathTrail('/srv/a\\b/c')).toEqual([
			{ label: '/', path: '/' },
			{ label: 'srv', path: '/srv' },
			{ label: 'a\\b', path: '/srv/a\\b' },
			{ label: 'c', path: '/srv/a\\b/c' }
		]);
	});

	it('ignores a trailing separator', () => {
		expect(pathTrail('/srv/docs/')).toEqual([
			{ label: '/', path: '/' },
			{ label: 'srv', path: '/srv' },
			{ label: 'docs', path: '/srv/docs' }
		]);
	});

	// コンテンツの登録先を選ぶ一覧は、「公開できるフォルダ」より上を辿れない (→ docs/folders.md「一覧 API」)。
	it('starts at the shared folder when one is given', () => {
		expect(pathTrail('/srv/media/2026/eiken', '/srv/media')).toEqual([
			{ label: 'media', path: '/srv/media' },
			{ label: '2026', path: '/srv/media/2026' },
			{ label: 'eiken', path: '/srv/media/2026/eiken' }
		]);
	});

	// 合わないときに空にすると、現在位置が画面から消える。切らずにそのまま出す。
	it('keeps the whole trail when the shared folder is not on it', () => {
		expect(pathTrail('/srv/docs', '/srv/media')).toEqual([
			{ label: '/', path: '/' },
			{ label: 'srv', path: '/srv' },
			{ label: 'docs', path: '/srv/docs' }
		]);
	});
});

describe('isSamePath', () => {
	it('ignores letter case', () => {
		expect(isSamePath('C:\\Share\\Docs', 'c:\\share\\docs')).toBe(true);
	});

	it('does not treat a parent as the same path', () => {
		expect(isSamePath('/srv', '/srv/docs')).toBe(false);
	});
});
