-- コンテンツの作成者。公開範囲 'private' (本人のみ) の判定にだけ使う (→ docs/design.md 5.1)。
-- NULL は作成者なし (このマイグレーションより前からある行と、作成者のユーザーが削除された行)。
-- 公開範囲に 'private' / 'hidden' が加わるが、visibility に CHECK 制約は置いていないため
-- スキーマの変更はこの列だけで済む (→ docs/design.md 10.1)。
--
-- ON DELETE は指定しない。ユーザーの削除は、その人の private を hidden に変えて作成者の記録を
-- 消してから行う (→ docs/design.md 7.4)。その手順を通らない削除は外部キー違反で失敗させ、
-- 作成者のいない private を作らない。
ALTER TABLE contents ADD COLUMN created_by INTEGER REFERENCES users (id);

CREATE INDEX idx_contents_created_by ON contents (created_by);
