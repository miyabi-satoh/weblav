-- 自動の再スキャンの決め方を、グループの AI のリンクの自動の更新 (`ai_link_groups.auto_mode` ほか) と同じ形にする。
-- 「1時間ごと」「1日ごと」の2つでは、NAS が空いている夜中の決まった時刻に回すことができなかったため。
-- `auto_rescan_mode` は 'off'・'interval' (前回から `auto_rescan_hours` 時間たったら)・'daily' (毎日 `auto_rescan_time` を過ぎたら)。
-- 選んだ決め方に要る値がそろっているかは、保存する側 (`schedule::parse_auto_refresh`) で確かめる。

ALTER TABLE contents ADD COLUMN auto_rescan_mode TEXT NOT NULL DEFAULT 'off';
ALTER TABLE contents ADD COLUMN auto_rescan_hours INTEGER;
-- 動かす PC の現地時刻の 'HH:MM'。
ALTER TABLE contents ADD COLUMN auto_rescan_time TEXT;

-- 今の間隔は、前回からの時間に移す。
UPDATE contents SET auto_rescan_mode = 'interval', auto_rescan_hours = 1 WHERE auto_rescan = 'hourly';
UPDATE contents SET auto_rescan_mode = 'interval', auto_rescan_hours = 24 WHERE auto_rescan = 'daily';

ALTER TABLE contents DROP COLUMN auto_rescan;
