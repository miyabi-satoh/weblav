// 本番の窓口に、結び付きの秘密を導く新しい X25519 の鍵を置く (→ docs/pro.md「結び付きと許可」・「アカウントと販売の窓口」)。
// 秘密鍵はディスクに書かず、標準入力で wrangler に渡す。控えは持たない。
// 鍵と link_kid は一度に置く (食い違った組で導かないように)。出力する公開鍵を src/pro.rs の LINK_KEYS (配る版) に足す。
// 鍵を替えると、前の鍵で結んだ WebLAV は確かめられなくなり、結び直してもらうことになる。
//
// 使い方: node scripts/new-link-key.mjs <link_kid (0〜255)>
import { byteRows, newKeySecrets, putSecrets } from './lib/new-key.mjs';

const [kid] = process.argv.slice(2);
if (!kid || !/^\d+$/.test(kid) || Number(kid) > 255) {
	console.error('usage: node scripts/new-link-key.mjs <link_kid (0-255)>');
	process.exit(1);
}

const { secrets, raw } = await newKeySecrets({
	algorithm: 'X25519',
	usages: ['deriveBits'],
	keyName: 'LINK_KEY',
	kidName: 'LINK_KID',
	kid
});
const entry = `(\n    ${kid},\n    [\n${byteRows(raw, 15, '        ')}\n    ],\n)`;
putSecrets({ wranglerArgs: ['secret', 'bulk'], secrets, list: 'LINK_KEYS', entry });
