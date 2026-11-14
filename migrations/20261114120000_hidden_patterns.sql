-- WebLAV のどこにも出さないもの (→ docs/folders.md「表示しないもの」) を、設置者が見て変えられるようにする。
-- 今まではドット始まりと Windows の隠し属性をコードで決めていて、何が出ないのかを管理者が知る手段が無かった。

-- サイト全体で、名前で表示しないもののパターン (1行に1つ)。NULL は既定のまま (`api::hidden::DEFAULT_SITE_PATTERNS`)。
-- 既定を DB に写さないのは、版を上げて既定を足したときに、変えていない設置にも届けるため。
ALTER TABLE site_settings ADD COLUMN hidden_patterns TEXT;
-- Windows の隠し属性の付いたものを表示しないか。名前のパターンでは書けないので、切り替えにする。
ALTER TABLE site_settings ADD COLUMN hide_windows_hidden INTEGER NOT NULL DEFAULT 1;

-- 共有ごとの除外のパターン (1行に1つ。共有の一番上から見る)。NULL は何も外さない。
ALTER TABLE roots ADD COLUMN exclude_patterns TEXT;
