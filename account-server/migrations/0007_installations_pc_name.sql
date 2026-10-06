-- 結んでいる WebLAV を一覧で見分けられるよう、その PC の名前 (mDNS のホスト名から `.local` を除いたもの) を持つ (→ docs/pro.md「窓口の画面」)。
-- サイト名を付けていない WebLAV はどれも同じ名前 (`WebLAV`) になり、名前だけでは見分けられないため。
-- 読めなかった PC は空のまま。確かめで空が届いたときは前の値を残す (名前と同じ扱い)。
ALTER TABLE installations ADD COLUMN pc_name TEXT NOT NULL DEFAULT '';
