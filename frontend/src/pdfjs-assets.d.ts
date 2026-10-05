// PDF.js が描画中に取りに行くファイルの置き場 (→ scripts/pdfjs-assets.ts)。末尾は `/`。
declare module 'virtual:pdfjs-assets' {
	const urls: Record<'cmaps' | 'wasm' | 'iccs', string>;
	export default urls;
}
