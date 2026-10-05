-- 組織向けは Stripe の請求書で売る (→ docs/pro.md「アカウントと販売の窓口」)。台帳の行は、Checkout Session か請求書のどちらか一方から作る。
-- SQLite は列の NOT NULL を外せないので、台帳を作り直す。
-- 台帳を指す Pro が残っていると古い台帳を消せない (外部キーの検査で断られる) ので、その間だけ退避する。
CREATE TABLE entitlements_saved AS SELECT * FROM entitlements WHERE purchase_id IS NOT NULL;
DELETE FROM entitlements WHERE purchase_id IS NOT NULL;

CREATE TABLE purchases_new (
    id TEXT PRIMARY KEY,
    account_id TEXT REFERENCES accounts (id) ON DELETE SET NULL,
    product TEXT NOT NULL,
    stripe_checkout_session_id TEXT UNIQUE,
    stripe_invoice_id TEXT UNIQUE,
    stripe_payment_intent_id TEXT NOT NULL UNIQUE,
    amount INTEGER NOT NULL,
    currency TEXT NOT NULL,
    managed_payments INTEGER,
    card_country TEXT,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER,
    detached_at INTEGER,
    CHECK ((stripe_checkout_session_id IS NULL) != (stripe_invoice_id IS NULL))
);
INSERT INTO purchases_new
    (id, account_id, product, stripe_checkout_session_id, stripe_payment_intent_id, amount, currency,
     managed_payments, card_country, created_at, revoked_at, detached_at)
SELECT id, account_id, product, stripe_checkout_session_id, stripe_payment_intent_id, amount, currency,
       managed_payments, card_country, created_at, revoked_at, detached_at
FROM purchases;
DROP TABLE purchases;
ALTER TABLE purchases_new RENAME TO purchases;
CREATE INDEX purchases_account ON purchases (account_id);
INSERT INTO entitlements SELECT * FROM entitlements_saved;
DROP TABLE entitlements_saved;

-- 組織向けのサポートの期限。組織向けの Pro に付き、Pro を移すと一緒に移る。
-- support_notice_for は、運営者に期限が近いと知らせた期限。延ばして期限が変わると、また知らせる。
ALTER TABLE entitlements ADD COLUMN support_until INTEGER;
ALTER TABLE entitlements ADD COLUMN support_notice_for INTEGER;
