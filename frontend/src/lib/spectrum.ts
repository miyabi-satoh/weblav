/** 棒に割り当てる周波数の下端と上端 (Hz)。声と楽器の聞こえる音がおさまる範囲。 */
const MIN_HZ = 60;
const MAX_HZ = 16000;

/**
 * AnalyserNode の周波数ごとの値 (`getByteFrequencyData`) を、`count` 本の棒の高さ (0〜1) にまとめる。
 * 耳は周波数の比で高さを聞き分けるので、棒の幅は対数で等しくする。均等に分けると、
 * 声や楽器の低い音が左の数本に詰まり、右の大半がほとんど動かない。
 */
export function spectrumBars(data: Uint8Array, sampleRate: number, count: number): number[] {
	const binHz = sampleRate / 2 / data.length;
	const ratio = Math.pow(MAX_HZ / MIN_HZ, 1 / count);
	return Array.from({ length: count }, (_, bar) => {
		const low = MIN_HZ * Math.pow(ratio, bar);
		const high = low * ratio;
		const first = Math.min(Math.floor(low / binHz), data.length - 1);
		// 低い棒は1つの bin より狭いので、少なくとも1つは受け持たせる。
		const last = Math.min(Math.max(Math.ceil(high / binHz), first + 1), data.length);
		let peak = 0;
		for (let i = first; i < last; i++) peak = Math.max(peak, data[i]);
		return peak / 255;
	});
}
