-- 軸ごとに、閲覧側の絞り込みに出すかを持つ(→ docs/design.md 3.2)。
--
-- 並び順や表示タイトルには使うが、閲覧者に選ばせる意味の無い軸(資料種別など)があるため。
-- 既存の軸はこれまでどおり出す。
ALTER TABLE archive_axes ADD COLUMN filterable INTEGER NOT NULL DEFAULT 1;
