/** 表示順 (`position`) を持つもの向け。今はアーカイブの軸だけが持つ。 */
export function byPosition(
	a: { position: number; id: number },
	b: { position: number; id: number }
): number {
	return a.position - b.position || a.id - b.id;
}
