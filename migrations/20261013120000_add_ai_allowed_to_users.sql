-- 編集者 (role が user) が AI の機能を使えるかの印 (→ docs/ai.md「誰が使えるか」)。
-- AI は管理者が買った残りを使うので、使わせる編集者を管理者が選ぶ。既定は使えない。
-- 管理者は印によらず使えるので、role が admin の行の値は判定に使わない (src/ai.rs の `is_allowed`)。
ALTER TABLE users ADD COLUMN ai_allowed INTEGER NOT NULL DEFAULT 0;
