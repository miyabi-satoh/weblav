-- リカバリコードを作った時刻を持つ。アカウントのページで「いつ作ったコードか」を出し、
-- 保存・印刷した控えの作成日時と見比べて、手元の控えが今使えるものかを分かるようにするため
-- (→ docs/access.md「リカバリコード」)。
-- コードを作る・消すときに recovery_code_hash と一緒に書く。この変更より前に作ったコードは
-- 時刻が分からないので NULL のまま残す (画面は「作成済み」だけを出す)。
ALTER TABLE users ADD COLUMN recovery_code_created_at TEXT;
