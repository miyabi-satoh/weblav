/**
 * 見出し・本体・ボタンを `<form>` で包むダイアログの、その `<form>` に付ける class。
 * `Dialog.Content` の縦の flex と段の間 (gap-6) をフォームの中でも続け、縮められる (min-h-0) ので
 * 画面の高さに届いたら `Dialog.Body` だけがスクロールする (→ components/ui/dialog/dialog-content.svelte)。
 */
export const dialogFormClass = 'flex min-h-0 flex-col gap-6';
