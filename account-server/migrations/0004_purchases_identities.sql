-- Stripe での購入の台帳 (→ docs/pro.md「アカウントと販売の窓口」)。購入ごとに1行で、返金・不審請求でも行は消さず、取り消した日時を入れる。
-- アカウントを消しても、結び付き (account_id) だけを外して7年残す (適格請求書の写しと帳簿の保存期間)。
-- 人を特定できる情報は持たない。
CREATE TABLE purchases (
    id TEXT PRIMARY KEY,
    account_id TEXT REFERENCES accounts (id) ON DELETE SET NULL,
    product TEXT NOT NULL,
    stripe_checkout_session_id TEXT NOT NULL UNIQUE,
    stripe_payment_intent_id TEXT NOT NULL UNIQUE,
    amount INTEGER NOT NULL,
    currency TEXT NOT NULL,
    -- 売上の区分のため。振り分けや返金には使わない。アカウントを消すと外す。
    managed_payments INTEGER,
    card_country TEXT,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER,
    -- アカウントを消して結び付きを外した日時。ここから保存の期間を数える。
    detached_at INTEGER
);
CREATE INDEX purchases_account ON purchases (account_id);

-- Pro は、買ったものなら台帳の行を指す。台帳と Pro を分けるのは、移すと持ち主が変わり、消すと Pro だけが消えるため。
-- これまで entitlements に持っていた Stripe の列は台帳へ移す。売り始める前で本番に行が無いので、検証用の行は捨てる。
DELETE FROM entitlements WHERE stripe_checkout_session_id IS NOT NULL;
DROP INDEX entitlements_checkout_session;
DROP INDEX entitlements_payment_intent;
ALTER TABLE entitlements DROP COLUMN stripe_checkout_session_id;
ALTER TABLE entitlements DROP COLUMN stripe_payment_intent_id;
ALTER TABLE entitlements DROP COLUMN stripe_managed_payments;
ALTER TABLE entitlements DROP COLUMN stripe_card_country;
ALTER TABLE entitlements ADD COLUMN purchase_id TEXT REFERENCES purchases (id);
CREATE UNIQUE INDEX entitlements_purchase ON entitlements (purchase_id);

-- 外部のサインイン (Google) の識別子。同じ provider と subject なら同じアカウント。
CREATE TABLE identities (
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (provider, subject)
);
CREATE INDEX identities_account ON identities (account_id);
