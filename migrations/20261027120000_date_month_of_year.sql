-- 日付の軸の 'date_month' を、年月 (`2024-06`) から年を問わない月 (`06`) に替える。
-- 年の軸と月の軸を組み合わせれば年月でも絞り込めるうえ、年月の軸では「どの年でも6月」を選べないため。
-- 抽出元の名前は変えないので軸の行はそのまま使える。値の辞書の行は年月の生の値に付けたもので、もう当たらないので消す。

DELETE FROM archive_axis_values
WHERE axis_id IN (SELECT id FROM archive_axes WHERE source = 'date_month');
