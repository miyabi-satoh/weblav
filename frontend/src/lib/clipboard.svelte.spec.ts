import { afterEach, beforeEach, expect, it } from 'vitest';
import { userEvent } from 'vitest/browser';
import { copyText } from './clipboard';

// LAN からのプレーン HTTP を真似て `navigator.clipboard` を隠し、execCommand の道を通す。
// 写したものは copy イベントの時点で選ばれている textarea から読む。
let copied: string[] = [];
function onCopy(e: Event) {
	const target = e.target;
	if (target instanceof HTMLTextAreaElement) {
		copied.push(target.value.slice(target.selectionStart, target.selectionEnd));
	}
}

beforeEach(() => {
	copied = [];
	Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
	document.addEventListener('copy', onCopy);
});

/** ボタンを押して写す。Chromium は利用者の操作から呼ばないと execCommand('copy') を断る。 */
async function clickToCopy(button: HTMLButtonElement, text: string) {
	let done: Promise<void> = Promise.resolve();
	button.onclick = () => (done = copyText(text));
	await userEvent.click(button);
	await done;
}

afterEach(() => {
	delete (navigator as { clipboard?: unknown }).clipboard;
	document.removeEventListener('copy', onCopy);
	document.body.replaceChildren();
});

it('ダイアログの外では、写してから押したボタンにフォーカスを戻す', async () => {
	const button = document.createElement('button');
	button.textContent = 'copy';
	document.body.appendChild(button);

	await clickToCopy(button, 'http://example.local:3000');

	expect(copied).toEqual(['http://example.local:3000']);
	expect(document.activeElement).toBe(button);
	expect(document.querySelector('textarea')).toBeNull();
});

it('ダイアログの中では、ダイアログの中に置いて写す', async () => {
	const dialog = document.createElement('div');
	dialog.setAttribute('role', 'dialog');
	const button = document.createElement('button');
	button.textContent = 'copy';
	dialog.appendChild(button);
	document.body.appendChild(dialog);
	let parent: Element | null = null;
	document.addEventListener('copy', (e) => (parent = (e.target as Element).parentElement), {
		once: true
	});

	await clickToCopy(button, 'AAAAA-BBBBB-CCCCC-DDDDD');

	expect(copied).toEqual(['AAAAA-BBBBB-CCCCC-DDDDD']);
	expect(parent).toBe(dialog);
	expect(document.activeElement).toBe(button);
});
