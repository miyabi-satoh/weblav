-- 時刻はすべて UNIX 秒。
CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    -- 小文字にそろえて持つ。どの方法で入っても、同じアドレスなら同じアカウント。
    email TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

-- メールのリンクでのサインイン。トークンはハッシュだけを持つ。
CREATE TABLE email_logins (
    token_hash TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    next TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX email_logins_email_created ON email_logins (email, created_at);

CREATE TABLE sessions (
    id_hash TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

-- アカウントが持つ Pro。買った記録ごとに1行。
CREATE TABLE entitlements (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts (id),
    plan TEXT NOT NULL CHECK (plan IN ('personal', 'organization')),
    created_at INTEGER NOT NULL
);
CREATE INDEX entitlements_account ON entitlements (account_id);

-- WebLAV からの申し込み (→ docs/pro.md「Pro にする流れ (WebLAV の側)」)。期限を過ぎたものは次の申し込みのときに消す。
CREATE TABLE activations (
    id TEXT PRIMARY KEY,
    poll_secret_hash TEXT NOT NULL,
    user_code TEXT NOT NULL UNIQUE,
    expires_at INTEGER NOT NULL,
    -- 認めたら埋まる。
    account_id TEXT REFERENCES accounts (id),
    plan TEXT CHECK (plan IN ('personal', 'organization')),
    approved_at INTEGER,
    created_at INTEGER NOT NULL
);
