/**
 * QR コードを見せる画面で共有する class。
 * QR コード自体の白地と quiet zone は変えず、外側に白いカードを1枚敷く
 * (ダークモードで白い画像だけが急に現れる印象を和らげる)。
 */

/** QR コードを載せるカード。 */
export const qrCardClass = 'rounded-lg border bg-white p-2';

/** QR コードの画像。スマートフォンのカメラで離れて読み取れる大きさ。 */
export const qrImageClass = 'size-48';
