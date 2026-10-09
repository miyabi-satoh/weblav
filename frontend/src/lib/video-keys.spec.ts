import { describe, expect, it } from 'vitest';
import { videoKeyAction } from './video-keys';

function key(key: string, init: Partial<Parameters<typeof videoKeyAction>[0]> = {}) {
	return videoKeyAction({
		key,
		target: { tagName: 'DIV' },
		ctrlKey: false,
		metaKey: false,
		altKey: false,
		shiftKey: false,
		isComposing: false,
		...init
	});
}

describe('videoKeyAction', () => {
	it('YouTube と同じキーを、画面のボタンの操作に割り当てる', () => {
		expect(key(' ')).toEqual({ type: 'toggle-play' });
		expect(key('k')).toEqual({ type: 'toggle-play' });
		expect(key('K', { shiftKey: false })).toEqual({ type: 'toggle-play' });
		expect(key('j')).toEqual({ type: 'rewind' });
		expect(key('l')).toEqual({ type: 'forward' });
		expect(key('m')).toEqual({ type: 'toggle-mute' });
		expect(key('f')).toEqual({ type: 'toggle-fullscreen' });
	});

	it('数字と Home・End は、全体の割合の位置へ移る', () => {
		expect(key('0')).toEqual({ type: 'seek-fraction', fraction: 0 });
		expect(key('7')).toEqual({ type: 'seek-fraction', fraction: 0.7 });
		expect(key('Home')).toEqual({ type: 'seek-fraction', fraction: 0 });
		expect(key('End')).toEqual({ type: 'seek-fraction', fraction: 1 });
	});

	it('左右キーはビューアの前後の移動に残すので扱わない', () => {
		expect(key('ArrowLeft')).toBeNull();
		expect(key('ArrowRight')).toBeNull();
	});

	it('ボタンの上のスペースはそのボタンに任せ、リンクの上では再生を切り替える', () => {
		expect(key(' ', { target: { tagName: 'BUTTON' } })).toBeNull();
		expect(key('k', { target: { tagName: 'BUTTON' } })).toEqual({ type: 'toggle-play' });
		expect(key(' ', { target: { tagName: 'A' } })).toEqual({ type: 'toggle-play' });
	});

	it('シークバーの上では効き、打ち込む欄の上では効かない', () => {
		expect(key(' ', { target: { tagName: 'INPUT', type: 'range' } })).toEqual({
			type: 'toggle-play'
		});
		expect(key('k', { target: { tagName: 'INPUT', type: 'text' } })).toBeNull();
		expect(key('k', { target: { tagName: 'DIV', isContentEditable: true } })).toBeNull();
	});

	it('修飾キー付きと変換中は横取りしない', () => {
		expect(key('f', { ctrlKey: true })).toBeNull();
		expect(key('f', { metaKey: true })).toBeNull();
		expect(key('f', { altKey: true })).toBeNull();
		expect(key('F', { shiftKey: true })).toBeNull();
		expect(key('k', { isComposing: true })).toBeNull();
	});
});
