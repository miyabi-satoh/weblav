// assets/icon.svg から、exeに埋め込む.ico (build.rs → winresource)・
// タスクトレイ用の生RGBA (src/tray/mod.rsがinclude_bytes!で読む)・MSIX のロゴ (scripts/msix.mjs)・
// ブラウザーのタブのアイコン (frontend の favicon。SVG は元をそのまま写す)・
// スマートフォンのホーム画面に置いたときのアイコン (frontend/static の apple-touch-icon・manifest の icon) を、
// assets/tray-icon-mac.svg から macOS のメニューバー用の生RGBAを生成する。
// sharp・to-icoはどちらもNode製で、Windows専用の外部ツール(Inkscape等)を要らない
// ため、Macでもこのまま実行できる。
import { copyFile, readFile, writeFile } from "node:fs/promises";
import sharp from "sharp";
import toIco from "to-ico";

const SVG_PATH = "assets/icon.svg";
const MAC_TRAY_SVG_PATH = "assets/tray-icon-mac.svg";
const FAVICON_PATH = "frontend/src/lib/assets/favicon.svg";
const ICO_PATH = "assets/icon.ico";
// exeに埋め込むアイコンのサイズ (Windowsの主な表示先: タスクバー・エクスプローラー各種・Alt+Tab)。
const ICO_SIZES = [16, 20, 24, 32, 48, 256];
// タスクトレイ用。実際の表示は16〜32px程度だが、高DPI環境での縮小表示をきれいにするため
// 大きめに持っておく (→ src/tray/mod.rs)。
const TRAY_SIZE = 64;
// MSIX のロゴ (installer/msix/AppxManifest.xml が参照する名前と、100% 表示でのサイズ)。
const MSIX_LOGOS = [
  ["StoreLogo", 50],
  ["Square44x44Logo", 44],
  ["Square150x150Logo", 150],
];
// ホーム画面のアイコン。iOS は 180px の apple-touch-icon を使い、Android (Chrome) は manifest の
// 192px と 512px を使う (それぞれの既定の大きさ)。
const STATIC_DIR = "frontend/static";
const APPLE_TOUCH_ICON_SIZE = 180;
const MANIFEST_ICON_SIZES = [192, 512];
// iOS は透けた所を黒く塗り、角は自分で丸めるので、apple-touch-icon は角の外も地の色で埋める。
const ICON_BACKGROUND = "#146E6B";
// SVGのviewBoxが32×32(px相当)なので、目的のサイズへ拡大する比率をdensityで指定する。
// sharpの既定density(72dpi)はviewBoxの数値をそのままpxとして扱うため、
// 32px相当を目的のサイズへ引き伸ばすには72 * (size/32) を指定すればよい。
const BASE_SIZE = 32;
const BASE_DENSITY = 72;

function densityFor(size) {
  return Math.round(BASE_DENSITY * (size / BASE_SIZE));
}

async function renderPng(svg, size) {
  return sharp(svg, { density: densityFor(size) })
    .resize(size, size)
    .png()
    .toBuffer();
}

async function renderIco(svg, sizes) {
  return toIco(await Promise.all(sizes.map((size) => renderPng(svg, size))));
}

async function renderRgba(svg, size) {
  return sharp(svg, { density: densityFor(size) })
    .resize(size, size)
    .ensureAlpha()
    .raw()
    .toBuffer();
}

async function main() {
  const svg = await readFile(SVG_PATH);

  await writeFile(ICO_PATH, await renderIco(svg, ICO_SIZES));
  console.log(`wrote ${ICO_PATH} (${ICO_SIZES.join("/")}px)`);

  const trayPath = `assets/tray-icon-${TRAY_SIZE}.rgba`;
  await writeFile(trayPath, await renderRgba(svg, TRAY_SIZE));
  console.log(`wrote ${trayPath} (${TRAY_SIZE}x${TRAY_SIZE}, raw RGBA)`);

  const macTrayPath = `assets/tray-icon-mac-${TRAY_SIZE}.rgba`;
  await writeFile(
    macTrayPath,
    await renderRgba(await readFile(MAC_TRAY_SVG_PATH), TRAY_SIZE),
  );
  console.log(`wrote ${macTrayPath} (${TRAY_SIZE}x${TRAY_SIZE}, raw RGBA)`);

  await copyFile(SVG_PATH, FAVICON_PATH);
  console.log(`wrote ${FAVICON_PATH}`);

  const appleTouchIcon = await sharp(svg, {
    density: densityFor(APPLE_TOUCH_ICON_SIZE),
  })
    .resize(APPLE_TOUCH_ICON_SIZE, APPLE_TOUCH_ICON_SIZE)
    .flatten({ background: ICON_BACKGROUND })
    .png()
    .toBuffer();
  await writeFile(`${STATIC_DIR}/apple-touch-icon.png`, appleTouchIcon);
  console.log(
    `wrote ${STATIC_DIR}/apple-touch-icon.png (${APPLE_TOUCH_ICON_SIZE}x${APPLE_TOUCH_ICON_SIZE})`,
  );
  for (const size of MANIFEST_ICON_SIZES) {
    const path = `${STATIC_DIR}/icon-${size}.png`;
    await writeFile(path, await renderPng(svg, size));
    console.log(`wrote ${path} (${size}x${size})`);
  }

  for (const [name, size] of MSIX_LOGOS) {
    const path = `assets/msix/${name}.png`;
    await writeFile(path, await renderPng(svg, size));
    console.log(`wrote ${path} (${size}x${size})`);
  }
}

main();
