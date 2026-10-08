/** 10秒戻す・進むの幅 (秒)。音声のプレイヤーと動画のビューアで揃える。 */
export const SKIP_SECONDS = 10;

/** シーク先を、頭から終わりまでに収める。長さがまだ分からなければ、終わり側は抑えない。 */
export function clampTime(seconds: number, duration: number): number {
	const end = duration > 0 ? duration : Number.POSITIVE_INFINITY;
	return Math.min(Math.max(seconds, 0), end);
}
