/**
 * 文字列をクリップボードに写す。失敗したら例外を投げる。
 *
 * Clipboard API はセキュアコンテキスト (HTTPS または localhost) でしか使えない。
 * LAN からのプレーン HTTP では `navigator.clipboard` 自体が無い。`execCommand('copy')` は
 * 非推奨だがセキュアコンテキストを問わず動くので、無いときだけこちらにフォールバックする。
 */
export async function copyText(text: string): Promise<void> {
	if (navigator.clipboard) {
		await navigator.clipboard.writeText(text);
	} else {
		legacyCopy(text);
	}
}

function legacyCopy(text: string) {
	const textarea = document.createElement('textarea');
	textarea.value = text;
	// readonly: 主な対象であるiPadのSafariは、フォーカスした編集可能な要素に
	// 仮想キーボードを出す。readonlyならフォーカスしても出ず、select()での
	// 選択・execCommand('copy')でのコピーはそのまま働く。
	textarea.setAttribute('readonly', '');
	textarea.style.position = 'fixed';
	textarea.style.opacity = '0';
	// ダイアログはフォーカスを中に閉じ込めるので、外に置くと select() でフォーカスが移らず、
	// 何も写らない (Chromium は成功を返す)。押したボタンのあるダイアログの中に置く。
	const previous = document.activeElement;
	const container = previous?.closest('[role="dialog"]') ?? document.body;
	container.appendChild(textarea);
	try {
		textarea.select();
		textarea.setSelectionRange(0, text.length);
		// フォーカスが移っていなければ、選んだ文字列ではなく別のものが写る。
		if (document.activeElement !== textarea) throw new Error('textarea not focused');
		const ok = document.execCommand('copy');
		if (!ok) throw new Error('execCommand copy failed');
	} finally {
		textarea.remove();
		// 戻さないと、ダイアログは消えた textarea の代わりに先頭の要素へフォーカスを移す。
		if (previous instanceof HTMLElement) previous.focus();
	}
}
