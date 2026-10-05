/**
 * 画面の骨組みで共有する class。個々の画面に同じ文字列を書かない。
 */
import { browseGutterClass } from '$lib/list-row';

/** 見出し (`<h1>`) の文字の大きさ。閲覧側・管理画面とも同じ大きさで置く (→ docs/ui.md「UI 全般」)。 */
export const pageHeadingTextClass = 'text-lg font-semibold';

/** 閲覧側の画面の見出し (`<h1>`)。管理画面の見出しの段は `adminPageHeaderClass` を使う。 */
export const pageHeadingClass = `mb-4 ${pageHeadingTextClass}`;

/**
 * 管理画面の見出しの段 (見出しと、横に並べるボタン)。中身との間は組と組の 24px (→ docs/ui.md「UI 全般」)。
 * 中の見出しには `pageHeadingTextClass` を使う。
 */
export const adminPageHeaderClass = 'mb-6 flex items-center justify-between';

/**
 * 管理画面の本文の外枠。左右の余白はタブ (`nav-tabs.ts`) と揃える。
 */
export const adminPageClass = 'p-4 md:px-6';

/**
 * 管理画面の一覧の表を包む帯。公開できるフォルダの一覧 (`listClass`) と同じく、本文の余白を越えて端から端まで
 * 面の色で塗り、上下に罫を引く (→ docs/ui.md「UI 全般」)。端の列の文字は本文の余白の線に揃える。
 * `adminPageClass` の中に置く。端の列の余白を変えたら、`layout.css` の `row-link-after-handle`・`row-link-before-handle` と、
 * コンテンツ管理の一覧の掴んだ行の複製の余白も合わせる。
 */
export const adminTableBandClass =
	'-mx-4 border-y bg-background md:-mx-6 [&_td:first-child]:pl-4 [&_td:last-child]:pr-4 [&_th:first-child]:pl-4 [&_th:last-child]:pr-4 md:[&_td:first-child]:pl-6 md:[&_td:last-child]:pr-6 md:[&_th:first-child]:pl-6 md:[&_th:last-child]:pr-6';

/** 一覧が空のときの文言。 */
export const pageEmptyTextClass = `${browseGutterClass} text-sm text-muted-foreground`;

/**
 * 読み物のページ (マニュアル・ライセンス) の外枠と、その見出しまわり。
 * 一覧を並べる画面と違い、本文を読ませるので幅を絞り、見出しも大きく取る。
 */
export const docPageClass = 'mx-auto w-full max-w-2xl px-6 py-10';

/** 見出しの上に置く小さな肩書き。 */
export const docPageEyebrowClass = 'text-xs tracking-wide text-muted-foreground';

/** 読み物のページの `<h1>`。 */
export const docPageHeadingClass = 'mt-2 text-2xl font-bold';

/** 見出しの下のリード文。 */
export const docPageLeadClass = 'mt-3 text-sm text-muted-foreground';

/** 枠で囲んだリスト。行の区切りは一覧と同じ `listItemClass` (`list-row.ts`) を使う。 */
export const docListClass = 'overflow-hidden rounded-lg border';

/**
 * マニュアル (`/help`) の外枠。タブレット以上では目次を左のサイドバーに固定し、
 * 本文を読み進めてもサイドバーが流れないようにする (→ docs/help.md)。
 * スマートフォン幅では並べる幅が無いので、`docPageClass` と同じ1段の縦並びのまま。
 * `docPageClass` (→ `/licenses` と共有) は幅を絞ったままにしたいので、
 * こちらは別に定義する。
 */
export const helpPageClass = 'mx-auto w-full max-w-5xl px-6 py-10 md:flex md:items-start md:gap-10';

/** マニュアルのサイドバー。ヘッダーの下に少し余白を置いて貼り付く。 */
export const helpSidebarClass = 'md:sticky md:top-20 md:w-56 md:shrink-0';

/** マニュアルの本文側。サイドバーと並ぶ幅では、縦並び用の上余白を消す。 */
export const helpContentClass = 'mt-10 min-w-0 flex-1 md:mt-0';

/**
 * カード1枚だけの画面 (ログイン・セットアップ) の外枠。
 *
 * 共通ヘッダー分を差し引いた残り高さではなく、ビューポート全体 (`fixed inset-0`) を基準に
 * 上下中央にする (ヘッダーの高さの分だけ見た目の中心が下にずれるのを避けるため)。
 * 共通ヘッダー (`+layout.svelte`) は `relative z-10` + `bg-background` でこの上に重なる。
 */
export const centeredPageClass = 'fixed inset-0 overflow-y-auto bg-ground';

/**
 * 上の枠の中で中央寄せするコンテナ。スクロール用 (`overflow-y-auto`) と分けてあるのは、
 * 同じ要素に付けると、低いビューポートでカードがビューポートより大きくなったときに
 * 上端へスクロールできず入力欄に到達できなくなるため。
 */
export const centeredPageInnerClass = 'flex min-h-full items-center justify-center p-6';

/** 上の中央寄せに置くカード。 */
export const centeredPageCardClass = 'w-full max-w-sm';

/** 上のカードの下端に置く、ほかの画面へのリンク。押せる範囲は上下に広げる。 */
export const centeredPageFooterLinkClass =
	'relative text-sm text-muted-foreground underline underline-offset-4 after:absolute after:inset-x-0 after:-inset-y-3';
