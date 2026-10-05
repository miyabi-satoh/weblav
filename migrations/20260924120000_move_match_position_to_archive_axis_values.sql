-- ファイル名の語を照合する位置を、軸から照合語の行ごとへ移す (→ docs/design.md 3.2)。
--
-- 同じ軸の中でも、ファイル名の付け方によって語を当てたい位置が違うため。
-- 今の軸の位置をその軸の各行へ写し、これまでの当たり方を変えない。
ALTER TABLE archive_axis_values ADD COLUMN match_position TEXT NOT NULL DEFAULT 'anywhere';
UPDATE archive_axis_values
   SET match_position = (SELECT match_position FROM archive_axes WHERE archive_axes.id = archive_axis_values.axis_id);
ALTER TABLE archive_axes DROP COLUMN match_position;
