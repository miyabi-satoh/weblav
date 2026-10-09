import { describe, expect, it } from 'vitest';
import { isLanUrl, remoteFileKind, remoteFileName, urlHost } from './file-kind';

describe('remoteFileKind', () => {
	it('パスの最後の部分の拡張子で種類を決める', () => {
		expect(remoteFileKind('https://pdfobject.com/pdf/sample.pdf')).toBe('pdf');
		expect(remoteFileKind('https://example.com/v/clip.MP4?token=abc#t=10')).toBe('video');
		expect(remoteFileKind('https://example.com/song.mp3')).toBe('audio');
		expect(remoteFileKind('https://example.com/photo.jpg')).toBe('image');
		expect(remoteFileKind('https://example.com/report.docx')).toBe('office');
	});

	it('ページ・ほかのファイル・LAN の URL はリンクのカードのままにする', () => {
		expect(remoteFileKind('https://example.com/')).toBeUndefined();
		expect(remoteFileKind('https://example.com/index.html')).toBeUndefined();
		expect(remoteFileKind('https://example.com/archive.zip')).toBeUndefined();
		expect(remoteFileKind('https://example.com/view?file=a.pdf')).toBeUndefined();
		expect(remoteFileKind('http://192.168.0.10/handout.pdf')).toBeUndefined();
		expect(remoteFileKind('http://nas.local/handout.pdf')).toBeUndefined();
	});
});

describe('remoteFileName', () => {
	it('% で符号化された名前を戻す', () => {
		expect(remoteFileName('https://example.com/%E8%B3%87%E6%96%99.pdf')).toBe('資料.pdf');
	});

	it('http・https でない URL は読まない', () => {
		expect(remoteFileName('ftp://example.com/a.pdf')).toBeUndefined();
		expect(remoteFileName('not a url')).toBeUndefined();
	});
});

describe('urlHost', () => {
	it('ポートがあれば付けたホスト名を返す', () => {
		expect(urlHost('https://example.com:8080/a?b=1')).toBe('example.com:8080');
	});

	it('読めない URL はそのまま返す', () => {
		expect(urlHost('not a url')).toBe('not a url');
	});
});

describe('isLanUrl', () => {
	it('公開でないアドレス・.local・ドットの無い名前は LAN とみなす', () => {
		for (const url of [
			'http://10.0.0.5/a.pdf',
			'http://172.20.1.1/a.pdf',
			'http://192.168.1.2/a.pdf',
			'http://127.0.0.1:8080/a.pdf',
			'http://169.254.10.10/a.pdf',
			'http://[::1]/a.pdf',
			'http://[fd00::1]/a.pdf',
			'http://nas.local/a.pdf',
			'http://nas/a.pdf'
		]) {
			expect(isLanUrl(url), url).toBe(true);
		}
	});

	it('公開のホストは LAN とみなさない', () => {
		expect(isLanUrl('https://pdfobject.com/pdf/sample.pdf')).toBe(false);
		expect(isLanUrl('http://8.8.8.8/a.pdf')).toBe(false);
		expect(isLanUrl('http://172.32.0.1/a.pdf')).toBe(false);
	});
});
