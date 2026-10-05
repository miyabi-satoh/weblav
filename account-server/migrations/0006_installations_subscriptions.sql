-- Pro を WebLAV の設置とアカウントの結び付きで扱い、サブスクで売る形にする (→ docs/pro.md)。
-- 売り始める前で本番に Pro の行が無いので、前の形 (申し込み・買い切りの Pro) の行は捨てる。

DROP TABLE activations;
DROP TABLE entitlements;

-- 結んでいる WebLAV。秘密は持たず、WebLAV の公開鍵から要るたびに導き直す。
-- 窓口で外した行は、WebLAV がそれを受け取る (確かめで unbound を返す) か、最後に出した許可の期限が過ぎるまで残し、台数に数える。
CREATE TABLE installations (
    -- 秘密から導いた結び付きの id (16進16文字)。
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    link_kid INTEGER NOT NULL,
    -- WebLAV の X25519 の公開鍵 (16進)。同じ申し込みを2度通しても同じ行にする。
    public_key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    checked_at INTEGER,
    removed_at INTEGER,
    permission_expires_at INTEGER
);
CREATE INDEX installations_account ON installations (account_id);

-- 外した結び付きの公開鍵。申し込みの期限 (24時間) が過ぎるまで、同じ申し込みでは結ばない。
CREATE TABLE released_keys (
    public_key TEXT PRIMARY KEY,
    released_at INTEGER NOT NULL
);

-- Pro。Stripe のサブスクごとに1行で、知らせはこの行で持ち主を引く。
-- アカウントを消しても行は持ち主だけを外して残し、後から届く知らせで Pro を付け直さない。
CREATE TABLE subscriptions (
    -- Stripe のサブスクの id (手元で付けた Pro は `dev-` で始まる)。
    id TEXT PRIMARY KEY,
    account_id TEXT REFERENCES accounts (id) ON DELETE SET NULL,
    plan TEXT NOT NULL CHECK (plan IN ('personal', 'organization')),
    stripe_customer_id TEXT,
    -- 払い終えた期間の終わり (UNIX 秒)。払われた請求書の期間の終わりで、古い請求書では縮めない。
    paid_through INTEGER NOT NULL,
    -- Stripe のサブスクの状態。canceled になったら戻さない。
    status TEXT NOT NULL,
    -- 返金・不審請求で打ち切った日時。打ち切った行は延ばさず、Pro にも数えない。
    revoked_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX subscriptions_account ON subscriptions (account_id);

-- 台帳は払われた請求書ごとの行にする。どのサブスクの請求書かも残す。
ALTER TABLE purchases ADD COLUMN stripe_subscription_id TEXT;

-- 支払いの画面の予約に、月額か年額かと、Stripe が作った Session の id を残す。
-- 別の払い方で押し直したときに、前の画面を閉じてから作り直すため。
ALTER TABLE checkouts ADD COLUMN price TEXT NOT NULL DEFAULT '';
ALTER TABLE checkouts ADD COLUMN session_id TEXT;
