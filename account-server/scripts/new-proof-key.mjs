// 本番の窓口に、証明に署名する新しい鍵を置く (→ docs/pro.md「結び付きと許可」・「アカウントと販売の窓口」)。
// 秘密鍵はディスクに書かず、標準入力で wrangler に渡す。控えは持たない。
// 鍵と kid は一度に置く (食い違った組で署名しないように)。出力する公開鍵を src/pro.rs の PROOF_KEYS (配る版) に足す。
//
// 使い方: node scripts/new-proof-key.mjs <kid> [--deploy]
//   --deploy: Worker がまだ無い最初の1回。secret を付けて wrangler deploy する。
import { byteRows, newKeySecrets, putSecrets } from './lib/new-key.mjs';

const [kid, flag] = process.argv.slice(2);
if (!kid || !/^[a-z0-9-]+$/.test(kid) || (flag !== undefined && flag !== '--deploy')) {
	console.error('usage: node scripts/new-proof-key.mjs <kid> [--deploy]  (kid: a-z, 0-9, -)');
	process.exit(1);
}

const { secrets, raw } = await newKeySecrets({
	algorithm: 'Ed25519',
	usages: ['sign', 'verify'],
	keyName: 'PROOF_SIGNING_KEY',
	kidName: 'PROOF_KID',
	kid
});
const entry = `    (\n        "${kid}",\n        [\n${byteRows(raw, 14, '            ')}\n        ],\n    ),`;
const wranglerArgs =
	flag === '--deploy' ? ['deploy', '--secrets-file', '/dev/stdin'] : ['secret', 'bulk'];
putSecrets({ wranglerArgs, secrets, list: 'PROOF_KEYS', entry });
