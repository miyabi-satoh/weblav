// `just linux-check-vm <VM> [テスト...]` の入口。Linux の確認 (linux-run.mjs) を VM の中で回す (e2e/vm/vm.sh)。
//
// 手元の画面・キーボード・フォーカスを使わず、Hyprland (arch-hyprland) と GNOME (ubuntu-2404・ubuntu-2604) を回せる。
// 回す前に、配る .deb を VM に入れる。Ubuntu は apt で、Arch は AUR と同じ PKGBUILD (packaging/aur/PKGBUILD.in) で
// パッケージを作って pacman で入れる。どちらも依存を引いて入れるので、配る形の依存だけで動くかも見る。
// .deb は、今のコミットと同じ中身で release-linux の workflow が作ったもの (`--deb <ファイル>` で別のものを指せる)。
// VM が止まっていれば画面なしで起動し、回し終えたら止める (`--keep` で動かしたまま)
import { execFile, spawn } from 'node:child_process';
import { createReadStream } from 'node:fs';
import fs from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';

const execFileAsync = promisify(execFile);
const e2eDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(e2eDir, '..');
const VM_SH = path.join(e2eDir, 'vm', 'vm.sh');
const VMS = ['arch-hyprland', 'ubuntu-2404', 'ubuntu-2604'];
const WORKFLOW = 'release-linux.yml';

const log = (message) => console.log(`[linux-check-vm] ${message}`);
const fail = (message) => {
	console.error(`[linux-check-vm] ${message}`);
	process.exit(1);
};

const args = process.argv.slice(2);
let keep = false;
let debArg = null;
const positional = [];
for (let i = 0; i < args.length; i++) {
	if (args[i] === '--keep') keep = true;
	else if (args[i] === '--deb') {
		debArg = args[++i];
		if (!debArg) fail('--deb には .deb のファイルを続けてください');
	} else if (args[i].startsWith('-'))
		fail(`知らない引数です: ${args[i]} (使えるのは --deb <ファイル>・--keep)`);
	else positional.push(args[i]);
}
const [vm, ...tests] = positional;
if (!VMS.includes(vm)) fail(`VM を選んでください: ${VMS.join('・')}`);

/** 標準出力とエラーをそのまま流して動かし、終了コードを返す。`input` は標準入力に流すストリーム */
function run(command, commandArgs, input) {
	return new Promise((resolve, reject) => {
		const child = spawn(command, commandArgs, {
			stdio: [input ? 'pipe' : 'ignore', 'inherit', 'inherit']
		});
		child.once('error', reject);
		child.once('exit', (code) => resolve(code ?? 1));
		if (input) input.pipe(child.stdin);
	});
}

const vmSh = (...a) => run(VM_SH, [vm, ...a]);
const ssh = (command, input) => run(VM_SH, [vm, 'ssh', command], input);

async function git(...a) {
	return (await execFileAsync('git', a, { cwd: repoRoot })).stdout.trim();
}

/** .deb に入らないファイル。変えても .deb を作り直さずに回せる */
const NOT_BUILT = [':!e2e', ':!.claude', ':!*.md', ':!justfile'];

/**
 * release-linux の workflow が作った .deb のうち、.deb に入るファイルが今のコミットと同じ回の、いちばん新しいものを落として、
 * そのパスを返す (e2e/ と文書だけを変えたコミットでは、作り直さずに前の回の .deb を使える)。当たる回が無ければ、作り方を書いて止まる。
 * .deb に入る変更をコミットしていなければ、.deb と食い違うので止まる
 */
async function ciDeb() {
	const dirty = (await git('status', '--porcelain', '--', '.', ...NOT_BUILT))
		.split('\n')
		.filter(Boolean)
		.map((line) => line.slice(3));
	if (dirty.length > 0) {
		fail(
			`コミットしていない変更があります (${dirty.slice(0, 3).join('、')}${dirty.length > 3 ? ' など' : ''})。` +
				'.deb はコミットから CI が作るので、コミットして push してから回してください'
		);
	}
	const { stdout } = await execFileAsync('gh', [
		'run',
		'list',
		'--workflow',
		WORKFLOW,
		'--status',
		'success',
		'--json',
		'databaseId,headSha',
		'--limit',
		'50'
	]);
	let found = null;
	for (const candidate of JSON.parse(stdout)) {
		// 手元に無いコミット (ほかのブランチの回など) は比べられない
		const known = await git('cat-file', '-e', `${candidate.headSha}^{commit}`).then(
			() => true,
			() => false
		);
		if (!known) continue;
		const same = await execFileAsync(
			'git',
			['diff', '--quiet', candidate.headSha, 'HEAD', '--', '.', ...NOT_BUILT],
			{ cwd: repoRoot }
		).then(
			() => true,
			() => false
		);
		if (same) {
			found = candidate;
			break;
		}
	}
	if (!found) {
		const branch = await git('rev-parse', '--abbrev-ref', 'HEAD');
		fail(
			`今のコミットと同じ中身の .deb を作った ${WORKFLOW} の回がありません。push してから ` +
				`gh workflow run ${WORKFLOW} --ref ${branch} で作り、終わってから回してください (作っている途中なら、終わるのを待つ)`
		);
	}
	const dir = path.join(repoRoot, 'target', 'linux-vm', found.headSha);
	const cached = (await fs.readdir(dir).catch(() => [])).find((name) => name.endsWith('.deb'));
	if (cached) return path.join(dir, cached);
	await fs.mkdir(dir, { recursive: true });
	log(`${WORKFLOW} の回 ${found.databaseId} (${found.headSha.slice(0, 8)}) の .deb を落とします`);
	await execFileAsync('gh', [
		'run',
		'download',
		String(found.databaseId),
		'--name',
		'deb',
		'--dir',
		dir
	]);
	const name = (await fs.readdir(dir)).find((n) => n.endsWith('.deb'));
	if (!name) fail(`回 ${found.databaseId} の成果物に .deb がありません`);
	return path.join(dir, name);
}

const deb = debArg ? path.resolve(debArg) : await ciDeb();
const debName = path.basename(deb);
const version = debName.match(/^weblav_(.+)_amd64\.deb$/)?.[1];
if (!version) fail(`.deb の名前が weblav_<版>_amd64.deb ではありません: ${debName}`);

/**
 * VM の中で .deb を入れるコマンド。VM は作ってから長く使うので、依存を引く前にパッケージの一覧を新しくする。
 * 入れた後、実行ファイルが読む共有ライブラリが揃っているかを見る
 */
function installCommand() {
	const ldd = `if ldd /usr/bin/weblav | grep 'not found'; then echo '共有ライブラリが足りません' >&2; exit 1; fi`;
	if (vm.startsWith('ubuntu')) {
		return `set -e; sudo apt-get update -qq; sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall /tmp/weblav-deb/${debName}; ${ldd}`;
	}
	// AUR に出す PKGBUILD を、この .deb を指すように埋めて作る (release-linux.yml の aur と同じ)
	return [
		'set -e',
		'cd /tmp/weblav-deb',
		`sed -e "s/@VERSION@/${version}/" -e "s/@SHA256_DEB@/$(sha256sum ${debName} | cut -d' ' -f1)/" PKGBUILD.in > PKGBUILD`,
		'makepkg --force --nodeps',
		// 依存は今のリポジトリから引く。Arch は一覧だけを新しくする入れ方 (-Sy) を許さないので、全体を上げる
		'sudo pacman -Syu --noconfirm',
		'sudo pacman -U --noconfirm weblav-bin-*.pkg.tar.zst',
		ldd
	].join('; ');
}

/**
 * 手元で tar に固めて、VM の中の `command` (標準入力から展開する) に流す。どちらかが落ちたら `failure` で投げる。
 * 手元の tar が途中で落ちても、VM の側は短い入力を受けて正常に終わることがあるので、両方の終わり方を見る
 */
async function sendTar(tarArgs, command, failure) {
	const child = spawn('tar', tarArgs, { cwd: repoRoot, stdio: ['ignore', 'pipe', 'inherit'] });
	const packed = new Promise((resolve) => {
		child.once('error', () => resolve(1));
		child.once('exit', (code) => resolve(code ?? 1));
	});
	const [remote, local] = await Promise.all([ssh(command, child.stdout), packed]);
	if (remote !== 0 || local !== 0) throw new Error(failure);
}

/** VM の中のシェルに渡す引数を引用する */
const quote = (arg) => `'${arg.replaceAll("'", "'\\''")}'`;

const wasRunning = await execFileAsync(VM_SH, [vm, 'ssh', 'true']).then(
	() => true,
	() => false
);
// Ctrl+C は子 (vm.sh・ssh) にも届いて終わらせるので、こちらは終わらずに後始末 (VM の停止) まで進む
process.on('SIGINT', () => {});
// 起動に失敗しても (中断も含む) qemu は裏で動き出していることがあるので止める
if (!wasRunning && (await vmSh('start', '--headless')) !== 0) {
	await vmSh('stop');
	process.exit(1);
}

let code = 1;
try {
	log(`配る .deb を入れます: ${deb}`);
	await sendTar(
		[
			'-c',
			'-C',
			path.dirname(deb),
			debName,
			'-C',
			path.join(repoRoot, 'packaging', 'aur'),
			'PKGBUILD.in'
		],
		'rm -rf /tmp/weblav-deb && mkdir -p /tmp/weblav-deb && tar -x -C /tmp/weblav-deb',
		'.deb を VM に写せませんでした'
	);
	if ((await ssh(installCommand())) !== 0) throw new Error('.deb を VM に入れられませんでした');
	// 確かめの仕組みを写す。手元で作られるもの (依存・ログ) は写さない
	await sendTar(
		['-c', '--exclude=e2e/node_modules', '--exclude=e2e/screenshots', 'e2e'],
		'rm -rf ~/weblav && mkdir -p ~/weblav && tar -x -C ~/weblav',
		'確かめの仕組みを VM に写せませんでした'
	);
	code = await ssh(`cd ~/weblav/e2e && node linux-run.mjs ${tests.map(quote).join(' ')}`);
} catch (error) {
	console.error(`[linux-check-vm] ${error.message}`);
} finally {
	if (!wasRunning && !keep) await vmSh('stop');
}
process.exitCode = code;
