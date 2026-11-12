import { describe, expect, it } from 'vitest';
import { isLanUrl, remoteFile, remoteFileKind, remoteFileName, urlHost } from './file-kind';

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

describe('remoteFile', () => {
	it('URL の拡張子で決まれば、応答で決めた名前より先に使う', () => {
		expect(remoteFile('https://example.com/a.pdf', 'b.mp4')).toEqual({
			kind: 'pdf',
			fileName: 'a.pdf'
		});
	});

	it('URL で決まらなければ、応答で決めた名前で決める', () => {
		expect(remoteFile('https://example.com/download?id=1', 'download.pdf')).toEqual({
			kind: 'pdf',
			fileName: 'download.pdf'
		});
		expect(remoteFile('https://example.com/song', 'song.mp3')?.kind).toBe('audio');
		expect(remoteFile('https://example.com/memo.txt', 'memo.txt')?.kind).toBe('text');
		expect(remoteFile('https://example.com/notes', 'notes.md')?.kind).toBe('text');
	});

	it('どちらでも決まらない・LAN の URL はリンクのカードのままにする', () => {
		expect(remoteFile('https://example.com/', null)).toBeUndefined();
		expect(remoteFile('https://example.com/memo.txt')).toBeUndefined();
		expect(remoteFile('https://example.com/x', 'x.zip')).toBeUndefined();
		expect(remoteFile('http://192.168.0.10/download', 'download.pdf')).toBeUndefined();
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
