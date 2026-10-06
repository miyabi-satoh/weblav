# 使っている部品のライセンス

配布物には第三者のソフトウェアが入るので、その表示を**アプリの中のページ `/licenses`** に出す。
**匿名で読める** (設置した人が誰でも確かめられるように)。

- 一覧は2つの生成物から作る。形はどちらも `{ texts: [{ id, name, text }], packages: [{ name, versions, license, repository, texts }] }`。
  抽出の結果は元データとして扱い、**画面に出す形は共通の `scripts/license-display.mjs` で整える**。
  - **1パッケージ1件**。同じ名前の違う版は1件にまとめて版を並べる。`license` は宣言しているライセンスの式。
  - 同じ条文は `texts` に1つだけ置き、パッケージからは添え字で指す。
  - 並びは名前の順。大文字と小文字を区別せず、文字コードで比べる (OS やロケールで生成物を変えないため)。
  - **本文が無い・ひな形のまま・ソースコードが混ざっている、のどれかが残ったら書き出さずに止める**。
  - **アプリ本体 (Rust) は cargo-about** (`about.toml`・`scripts/generate-rust-licenses.mjs`)。
    `just licenses` で `frontend/static/third-party-licenses/rust.json` を作り、**リポジトリに入れる** (`just licenses-check` がずれを見る)。
    結果が環境で変わらないよう、**生成そのものは**ネットを見ず手元のレジストリだけから作る (`--frozen` = `--locked --offline`)。
    引き換えに、本文を同梱していないクレートは SPDX のひな形 (`Copyright (c) <year> <copyright holders>`) に落ちる。
    そのクレートは、上流の公開時のコミットから取った条文を `license-texts/` に置き、版を指定して差し替える (スクリプトの `OVERRIDES`)。
    同梱しているのに読み取られないクレートは、`about.toml` の `clarify` でファイルを指す。
    版が上がって差し替えが外れたときも、上の検査で止まるので気づける。
    表示するのは**実際にリンクされるクレートだけ** (`cargo tree -e normal,no-proc-macro` を `about.toml` の `targets` ごとに)。
    cargo-about はビルドのときにだけ動く proc-macro とその依存も数えるが、それらは配らない。
    `--frozen` の前に `cargo fetch --locked` を流す。cargo-about が内部で呼ぶ `cargo metadata` は
    どのプラットフォーム向けの依存も手元に要求し、Windows でビルドしただけのレジストリには
    Linux 向けの `gtk` などが無いため。lockfile が指すものだけを取るので、結果は変わらない。
    出力は標準出力ではなく `-o` で一時ファイルに受ける。cargo-about は先祖に PowerShell がいると
    標準出力への書き出しを拒むので、Windows では `just licenses` がそのままでは通らない。
    `about.toml` の `accepted` に無いライセンスの依存が入ると生成が止まるので、そこで気づける。
  - **画面 (npm) は Vite プラグイン** (`frontend/scripts/third-party-licenses.ts`) が、ビルドのたびに `build/third-party-licenses/npm.json` を出す。
    `package.json` の依存ではなく、実際に chunk とアセットに入ったパッケージだけを数える。lint やテストの道具を表示に混ぜないため。
    Vite・Rolldown がバンドルに差し込む仮想モジュール (ID が `\0` で始まる) は `VIRTUAL_MODULES` で持ち主のパッケージに結び付け、知らないものが入ったら止める。
    chunk から辿れないものは `EXTRA_PACKAGES` に明示して足す。Paraglide の生成物は `src/lib/paraglide` に出て node_modules の外になり、
    CSS の `@import` (Tailwind ほか) は `@tailwindcss/vite` が展開するのでモジュールにもアセットにも現れないため。
    ライセンスの id が `ACCEPTED` に無ければビルドを止める (Rust 側の `about.toml` の `accepted` と同じ役目)。
    SPDX の式は、全体を囲む括弧を除いて入れ子があれば読まずに止める。括弧を外して読むと意味が変わるため。
    本文を同梱していないパッケージもビルドを止める。名前だけ並べても許諾表示を配ったことにならないので、同じ条文の借り先を `BORROWED_TEXTS` に足す。
- ページは**1パッケージ1行**のリスト (名前・版・ライセンスの式)。**行を開くと、ソースの置き場所と条文を出す**。
  数百件あり、既定で開くと目当てのパッケージに辿り着けないため。
  ライセンスごとの見出しの下には束ねない。目当てのパッケージの条文を探すのに、どの束にいるかを知っている必要があるため。
- **ソースの置き場所はリンクで出す**。
  MPL-2.0 の依存 (`option-ext`) が入っており、受け取る人にソースの入手先を知らせる必要があるため。
  リンクは開閉の `summary` の外 (開いた中身) に置く。中に置くと、押したときに開閉とリンクのどちらが働くかが紛らわしい。
  `repository` を書いていないパッケージは出さない。
  リンクは**高さを 44px にする** (→ ui.md「UI 全般」。WCAG 2.2 AA の 24px も満たす)。
- 導線は**マニュアルの目次の下**。マニュアルの1ページではないので、目次の中には入れない。
- **開発サーバーには npm の分が無い** (ビルドの成果物として作るため)。取れなかった側はその旨だけを出し、ページ自体は出す。
