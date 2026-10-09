/**
 * 閲覧側のパンくず (グループ・フォルダー) で共有する class。
 * リンクは実際の高さを 44px にし (→ docs/ui.md「UI 全般」)、増えた上下の余白を nav の負の margin で
 * 打ち消して文字の位置を保つ。2つの値は互いに依存しているので、片方だけ変えないこと。
 */
export const breadcrumbNavClass =
	'-mt-3 mb-1 flex flex-wrap items-center gap-x-1 text-sm text-muted-foreground';

export const breadcrumbLinkClass = 'inline-flex h-11 items-center underline underline-offset-4';

/** フォルダーの中のパス ('a/b/c') を、パンくずの段 ('a'・'a/b'・'a/b/c' までの累積) に分ける。 */
export function pathCrumbs(path: string): { name: string; path: string }[] {
	const segments = path.split('/').filter((segment) => segment.length > 0);
	return segments.map((name, index) => ({ name, path: segments.slice(0, index + 1).join('/') }));
}
