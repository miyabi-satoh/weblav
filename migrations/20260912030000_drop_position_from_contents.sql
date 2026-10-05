-- コンテンツから表示順(position)を落とす。並びは閲覧する人が「タイトル順」「新しい順」
-- から選ぶ方式に変える(→ docs/requirements.md 4.4、docs/design.md 9.4)。
--
-- 手で並べる方式をやめる理由は要件に書いたとおりで、並べ替えの手間が管理者に掛かり続け、
-- 見る人によって見えない行(private/hidden)が混ざるため順番の意味が揃わない。
--
-- 並べ替えはSQLではなくアプリケーション層で行う(「第2回」を「第10回」より前に置くため、
-- 数字を数値として比べる必要がある)。そのため ORDER BY 用のインデックスは作り直さない。
-- idx_contents_parent_id は WHERE parent_id = ? / IS NULL の絞り込みに今も要るので、
-- position を除いた形で作り直す。
--
-- SQLiteはインデックスに含まれる列をDROPできないため、先に両方のインデックスを落とす。
DROP INDEX idx_contents_position;
DROP INDEX idx_contents_parent_id;

ALTER TABLE contents DROP COLUMN position;

CREATE INDEX idx_contents_parent_id ON contents (parent_id, id);
