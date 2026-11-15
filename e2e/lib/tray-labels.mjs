// トレイのメニューの文言。src/tray/mod.rs の TrayLocale の文言と同じものを並べる (変えたら両方を直す)
export const TRAY_LABELS = {
	ja: {
		setup: 'セットアップ',
		open: 'ブラウザーで開く',
		manual: 'マニュアル',
		loginItem: 'ログイン時に起動',
		loginItemNeedsApproval: 'ログイン時に起動 (システム設定で許可してください)',
		signInItem: 'サインイン時に起動',
		signInItemLocked: 'サインイン時に起動 (Windows の設定で止められています)',
		quit: '終了する',
		portMoved: (configured, actual) =>
			`ポート ${configured} を使えないため、${actual} で動いています`
	},
	en: {
		setup: 'Setup',
		open: 'Open in browser',
		manual: 'Manual',
		loginItem: 'Start at login',
		loginItemNeedsApproval: 'Start at login (allow it in System Settings)',
		signInItem: 'Start at sign-in',
		signInItemLocked: 'Start at sign-in (turned off in Windows Settings)',
		quit: 'Quit',
		portMoved: (configured, actual) => `Port ${configured} was unavailable; running on ${actual}`
	}
};

/** OS の優先する言語 (`ja-JP` など) から、トレイの表示言語を決める (src/tray/mod.rs の locale_from_name と同じ) */
export function labelsFor(languageTag) {
	return languageTag.split(/[-_.@]/)[0].toLowerCase() === 'ja' ? TRAY_LABELS.ja : TRAY_LABELS.en;
}

/** Windows の表示言語の LANGID (`GetUserDefaultUILanguage`) から決める (src/tray/mod.rs の locale_from_langid と同じ) */
export function labelsForLangId(langId) {
	// LANG_JAPANESE。下位10ビットが主の言語
	return (langId & 0x3ff) === 0x11 ? TRAY_LABELS.ja : TRAY_LABELS.en;
}
