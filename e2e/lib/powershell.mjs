// PowerShell のスクリプトを流す。Windows の確認 (`just msix-check`) で、OS の側 (レジストリ・窓・トレイ・ごみ箱) を読み書きするのに使う。
import { spawn } from 'node:child_process';

/**
 * PowerShell に `script` を渡す引数を作る。
 *
 * 素朴に `-Command <script>` で日本語を渡すと、コンソールのコードページ (既定では
 * Shift_JIS のことが多い) 経由でエンコードが化ける。`-EncodedCommand` (UTF-16LE を
 * Base64 にしたもの) で渡し、標準出力のエンコーディングも明示的に UTF-8 にすることで避ける。
 * 進捗の表示 (Add-Type などが出す) は、標準エラーを繋いだままだと CLIXML として流れ込み、
 * 本来のエラーの文言を埋もれさせるので止める。
 * 終わるまで待たずに動かし続けるプロセス (spawn) にも使う
 */
export function powerShellArgs(script) {
	const wrapped = `[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n$ProgressPreference = 'SilentlyContinue'\n$ErrorActionPreference = 'Stop'\n${script}`;
	const encoded = Buffer.from(wrapped, 'utf16le').toString('base64');
	return ['-NoProfile', '-EncodedCommand', encoded];
}

// 呼ぶたびに PowerShell を起動すると、起動だけで 0.4 秒ほどかかり、Add-Type の C# のコンパイルも毎回やり直しになる
// (トレイのメニューを読み直す待ちのように、何度も呼ぶ所で積み重なる)。PowerShell を1つ立ち上げたままにし、スクリプトを標準入力へ
// 1行ずつ流して、結果を標準出力の1行で受け取る。読み込んだ型はプロセスに残り、同じ定義の Add-Type は2回目から何もしない
// (定義が違うとエラーになるので、同じ名前の型を別の中身で定義し直さないこと)。
// **ハンドルや座標など毎回変わる値は、スクリプトに埋め込まず `args` で渡す** (スクリプトの中では `$args[0]` などで読む)。
// スクリプトは中身ごとに検査され、C# のソースを含むスクリプトは中身が変わるたびに 0.5〜1 秒かかる (同じ中身なら覚えていて数 ms。
// Windows の AMSI と見られる)。値を埋め込むと、クリックのたびに検査し直しになる。
// スクリプトと `args` は JSON にし、UTF-8 を Base64 にして渡す。結果は「@@e2e」の印、「ok か error」、UTF-8 を Base64 にした本文を1行で返す
// (改行や日本語を1行に収めるため)。
// 結果は標準出力を使って返すので、スクリプトの中で Write-Host や [Console]::Out に書かないこと (印のない行は、読み飛ばして警告を出す)。
// スクリプトは呼ぶたびに子のスコープで動くので、変数は次の呼び出しに残らない。
// 立ち上げたプロセスは標準入力が閉じると終わる。テストのプロセスが落ちたり強制終了されたりしてもパイプが閉じるので、残らない
const HOST_SCRIPT = `
# 警告などは、出力をパイプにしていると標準出力に混ざり、応答の行と取り違えるので出さない
$WarningPreference = 'SilentlyContinue'
$VerbosePreference = 'SilentlyContinue'
$InformationPreference = 'SilentlyContinue'
$utf8 = [System.Text.UTF8Encoding]::new($false)
$blocks = @{}
while ($true) {
    $line = [Console]::In.ReadLine()
    if ($null -eq $line) { break }
    try {
        $request = $utf8.GetString([Convert]::FromBase64String($line)) | ConvertFrom-Json
        if (-not $blocks.ContainsKey($request.script)) { $blocks[$request.script] = [ScriptBlock]::Create($request.script) }
        $arguments = @($request.args)
        $items = @(& $blocks[$request.script] @arguments)
        $text = ($items | ForEach-Object { if ($_ -is [string]) { $_ } else { ($_ | Out-String -Width 4096).TrimEnd() } }) -join "\`n"
        if ($items.Count -gt 0) { $text += "\`n" }
        $status = 'ok'
    } catch {
        $text = $_ | Out-String -Width 4096
        $status = 'error'
    }
    [Console]::Out.WriteLine('@@e2e ' + $status + ' ' + [Convert]::ToBase64String($utf8.GetBytes($text)))
    [Console]::Out.Flush()
}
`;

/** 立ち上げたままの PowerShell。まだ起動していないか、終わった後は null */
let host = null;
/** 1つのプロセスに1つずつ流すための順番待ち */
let queue = Promise.resolve();

function startHost() {
	const child = spawn('powershell', powerShellArgs(HOST_SCRIPT), {
		stdio: ['pipe', 'pipe', 'inherit'],
		windowsHide: true
	});
	const self = { child, pending: [] };
	let buffered = '';
	child.stdout.setEncoding('utf8');
	child.stdout.on('data', (chunk) => {
		buffered += chunk;
		for (let index = buffered.indexOf('\n'); index >= 0; index = buffered.indexOf('\n')) {
			const line = buffered.slice(0, index).trim();
			buffered = buffered.slice(index + 1);
			const [marker, status, payload = ''] = line.split(' ');
			if (marker !== '@@e2e') {
				// スクリプトが標準出力に直接書いた行。応答と取り違えて、以降の対応がずれないよう読み飛ばす
				if (line) console.warn(`[e2e] PowerShell が応答以外の行を出しました: ${line}`);
				continue;
			}
			const request = self.pending.shift();
			if (self.pending.length === 0) child.stdout.unref();
			if (!request) continue;
			const text = Buffer.from(payload, 'base64').toString('utf8');
			if (status === 'ok') request.resolve(text);
			else request.reject(new Error(`PowerShell のスクリプトが失敗しました: ${text.trim()}`));
		}
	});
	const fail = (error) => {
		if (host === self) host = null;
		for (const request of self.pending.splice(0)) request.reject(error);
	};
	child.once('error', fail);
	child.once('exit', (code, signal) =>
		fail(new Error(`立ち上げたままの PowerShell が終わりました (code=${code}, signal=${signal})`))
	);
	// 待っている間に ref しているのは標準出力だけなので、落ちたときは exit より先に閉じたことで気づく
	// (unref した child の exit を待っていると、届く前にイベントループが空になってテストのプロセスが終わることがある)
	child.stdout.once('close', () =>
		fail(new Error('立ち上げたままの PowerShell の標準出力が閉じました'))
	);
	// Ctrl+C は同じコンソールの PowerShell にも届き、先に終わっていることがある。そのとき書き込もうとして落ちないようにする
	child.stdin.on('error', () => {});
	// 待っている結果がないときは、テストのプロセスがこの子を待たずに終われるようにする (終わればパイプが閉じ、PowerShell も終わる)
	child.unref();
	child.stdin.unref();
	child.stdout.unref();
	return self;
}

function send(script, args) {
	host ??= startHost();
	const current = host;
	return new Promise((resolve, reject) => {
		current.pending.push({ resolve, reject });
		current.child.stdout.ref();
		const request = Buffer.from(JSON.stringify({ script, args }), 'utf8').toString('base64');
		current.child.stdin.write(`${request}\n`);
	});
}

/**
 * PowerShell でスクリプトを実行し、出力を文字列で返す (出力があれば、末尾に改行が1つ付く)。失敗したら、PowerShell のエラーの文言で失敗させる。
 * 毎回変わる値は `args` (文字列か数値の配列) で渡し、スクリプトの中では `$args[0]` などで読む (上の HOST_SCRIPT の説明を参照)
 *
 * @param {string} script
 * @param {(string | number)[]} [args]
 */
export function runPowerShell(script, args = []) {
	const result = queue.then(() => send(script, args));
	queue = result.catch(() => {});
	return result;
}

/** 立ち上げたままの PowerShell を終わらせ、終わるまで待つ (テストファイルの後始末で呼ぶ。呼ばなくても、テストのプロセスが終われば終わる) */
export async function stopPowerShell() {
	const current = host;
	if (!current) return;
	host = null;
	const { child } = current;
	if (child.exitCode !== null || child.signalCode !== null) return;
	const exited = new Promise((resolve) => child.once('exit', resolve));
	child.ref();
	child.stdin.end();
	await exited;
}

/** OS のグローバルホットキーとして Ctrl+Shift+Space を送る (SendKeys 経由。RegisterHotKey はフォーカスに関係なく反応する) */
