-- AI を誰がどれだけ使ったかの記録 (→ docs/ai.md「使った量の記録」)。窓口へ呼び出すたびに1行。
-- 残りは設置で1つを共有し、窓口は誰が使ったかを知らないので、内訳はここにだけ残る。
--
-- user_id はユーザーを消すと NULL になる。消した後も内訳に名前を出せるよう、使った時点のユーザー名を username に写す。
-- kind は使った機能 ('axes')。値の検証は src/ai.rs の `UsageKind` で行う。
-- used は窓口が返した量で、1回の購入で付く量を 1,000,000 とした整数。
CREATE TABLE ai_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER REFERENCES users (id) ON DELETE SET NULL,
    username TEXT NOT NULL,
    kind TEXT NOT NULL,
    used INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
-- ユーザーごとの合計を引くため。
CREATE INDEX ai_usage_user_id ON ai_usage (user_id);
