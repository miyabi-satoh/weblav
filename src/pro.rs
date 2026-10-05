//! Pro の結び付きと許可 (→ docs/pro.md「結び付きと許可」)。設定とデータの置き場の `pro.json` に持ち、
//! 許可 (署名付きの証明か返しのコード) を確かめて Free か Pro かを決める。窓口とのやり取りは `api::pro`。

pub mod link;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const DAY: i64 = 86_400;
/// 時計の戻りとして許す幅。タイムゾーンの設定の直しや、時刻合わせのずれで Free にしないように。
const CLOCK_TOLERANCE: i64 = DAY;
/// 見た最も新しい時刻をファイルに書く間隔。要求のたびに書かないように。
const HIGH_WATER_PERSIST: i64 = 3600;
/// 許可があるときに確かめる間隔 (→ docs/pro.md「確かめる」)。窓口で外した・解約したことを、この長さのうちに反映する。
const CHECK_INTERVAL: i64 = 30 * DAY;
/// 期限がこれより近いと、間隔によらず確かめる。払い終えた期間の更新を、期限が切れる前に受け取るため。
const CHECK_BEFORE_EXPIRY: i64 = 7 * DAY;
/// 許可が無い結び付き (Pro が無い・上限を超えた・期限切れ) を確かめる間隔。買い直したら、次に使われたときに戻るように。
const CHECK_WITHOUT_PERMISSION: i64 = 3600;

/// 動いている WebLAV が Free か Pro か。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Edition {
    Free,
    Pro,
}

impl Edition {
    pub fn of(plan: Option<Plan>) -> Self {
        match plan {
            Some(_) => Self::Pro,
            None => Self::Free,
        }
    }
}

/// Pro の種類。台数の上限と画面の表示に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Plan {
    Personal,
    Organization,
}

/// 窓口が直近に許可を出さなかったわけ。区画に、どうすれば戻るかを出すため。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    /// アカウントに Pro が無い (解約・返金など)。
    NoPlan,
    /// 台数の上限を超えた分。
    OverLimit,
}

/// 許可の受け取り方。ネットで受け取った署名付きの証明か、打ち込んだ返しのコードか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PermissionKind {
    Online,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Permission {
    Proof { value: String },
    Code { value: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Binding {
    installation: String,
    link_kid: u8,
    /// 秘密 (base64url)。
    secret: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    permission: Option<Permission>,
    #[serde(default)]
    last_checked_at: Option<i64>,
    #[serde(default)]
    refusal: Option<Refusal>,
}

/// 結びかけの申し込み。返しのコードを打ち込む前に起動し直しても続けられるよう、ファイルに残す。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingLink {
    link_kid: u8,
    secret: String,
    request: String,
    created_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProFile {
    #[serde(default)]
    binding: Option<Binding>,
    #[serde(default)]
    pending: Option<PendingLink>,
    /// 見た最も新しい時刻。時計を戻して期限を延ばせないように (→ docs/pro.md「結び付きと許可」の「時計」)。
    #[serde(default)]
    clock_high_water: i64,
}

impl ProFile {
    fn is_empty(&self) -> bool {
        self.binding.is_none() && self.pending.is_none()
    }
}

/// 確かめた許可。
#[derive(Debug, Clone, Copy)]
struct Verified {
    plan: Plan,
    expires_at: i64,
    kind: PermissionKind,
}

#[derive(Debug, Default)]
struct State {
    file: ProFile,
    verified: Option<Verified>,
    /// ファイルに書いた見た最も新しい時刻。
    persisted_high_water: i64,
}

impl State {
    fn high_water_due(&self) -> bool {
        !self.file.is_empty()
            && self.file.clock_high_water >= self.persisted_high_water + HIGH_WATER_PERSIST
    }
}

/// 動いている WebLAV の Pro の状態。起動したときに `pro.json` を読み、結ぶ・確かめる・外すと変わる。
#[derive(Debug)]
pub struct Pro {
    path: PathBuf,
    // ファイルの書き換えの間も持ち続ける。ファイルとこの値が食い違わないように。
    state: Mutex<State>,
}

/// 区画に出す今の状態。
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub plan: Option<Plan>,
    pub bound: bool,
    pub email: Option<String>,
    pub expires_at: Option<i64>,
    pub kind: Option<PermissionKind>,
    pub last_checked_at: Option<i64>,
    pub refusal: Option<Refusal>,
    pub clock_behind: bool,
    /// 結びかけの申し込み。
    pub pending_request: Option<String>,
}

/// 窓口に確かめる相手。結んである結び付きか、結びかけの申し込み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckTarget {
    Bound(String),
    Pending(String),
}

/// 窓口に確かめるのに要るもの。
#[derive(Debug, Clone)]
pub struct CheckRequest {
    pub target: CheckTarget,
    pub installation: String,
    pub auth: String,
}

/// 窓口の確かめの答え (→ docs/pro.md「窓口との形」)。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CheckOutcome {
    Ok { proof: String, email: String },
    NoPlan { email: String },
    OverLimit { email: String },
    Unbound,
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("failed to get random bytes")]
    Random(#[source] getrandom::Error),
    #[error("failed to write the Pro file")]
    Write(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum CodeError {
    #[error("no pending link")]
    NotStarted,
    #[error("invalid code")]
    Invalid,
    #[error("the clock is behind the code")]
    ClockBehind,
    #[error("failed to write the Pro file")]
    Write(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("invalid Pro proof: {0}")]
    Invalid(#[source] Error),
    #[error("the clock is behind the proof")]
    ClockBehind,
    #[error("failed to write the Pro file")]
    Write(#[from] std::io::Error),
}

/// 証明の確かめの失敗。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("malformed proof")]
    Malformed,
    #[error("unsupported proof version {0}")]
    UnsupportedVersion(u32),
    #[error("unknown key id {0:?}")]
    UnknownKey(String),
    #[error("bad signature")]
    BadSignature,
    #[error("proof is for another installation")]
    OtherInstallation,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Pro {
    /// `pro_path` を読んで、Free か Pro かを決める。
    /// 無ければ Free。読めない・確かめられないときも、その許可は無いものとして動き、ログに残す。
    pub fn load(pro_path: &Path) -> Self {
        let read = match std::fs::read(pro_path) {
            Ok(bytes) => serde_json::from_slice::<ProFile>(&bytes).map_err(std::io::Error::other),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ProFile::default()),
            Err(err) => Err(err),
        };
        let file = read.unwrap_or_else(|err| {
            tracing::warn!(%err, path = %pro_path.display(), "failed to read the Pro file; running as Free");
            ProFile::default()
        });
        let verified = file.binding.as_ref().and_then(verify_binding);
        if let Some(v) = verified {
            tracing::info!(plan = ?v.plan, expires_at = v.expires_at, kind = ?v.kind, "loaded Pro permission");
        }
        let persisted_high_water = file.clock_high_water;
        Self {
            path: pro_path.to_path_buf(),
            state: Mutex::new(State {
                file,
                verified,
                persisted_high_water,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub fn edition(&self) -> Edition {
        Edition::of(self.plan())
    }

    /// 今、Pro として動くか。許可の期限が過ぎた・時計が戻っているときは `None`。
    pub fn plan(&self) -> Option<Plan> {
        let now = now();
        let mut state = self.lock();
        observe(&mut state, now);
        active_plan(&state, now)
    }

    pub fn snapshot(&self) -> Snapshot {
        let now = now();
        let mut state = self.lock();
        observe(&mut state, now);
        let binding = state.file.binding.as_ref();
        Snapshot {
            plan: active_plan(&state, now),
            bound: binding.is_some(),
            email: binding.and_then(|b| b.email.clone()),
            expires_at: state.verified.map(|v| v.expires_at),
            kind: state.verified.map(|v| v.kind),
            last_checked_at: binding.and_then(|b| b.last_checked_at),
            refusal: binding.and_then(|b| b.refusal),
            clock_behind: clock_behind(&state, now),
            pending_request: state.file.pending.as_ref().map(|p| p.request.clone()),
        }
    }

    /// 結びかけの申し込みを作る。前の結びかけは捨てる。結んであれば、同じ枠の結び直しにする。
    pub fn start_link(&self) -> Result<String, LinkError> {
        let now = now();
        let (link_kid, server_public) = LINK_KEYS[LINK_KEYS.len() - 1];
        let mut client_secret = [0u8; 32];
        getrandom::fill(&mut client_secret).map_err(LinkError::Random)?;
        let mut state = self.lock();
        let previous = state
            .file
            .binding
            .as_ref()
            .and_then(|b| decode_secret(&b.secret));
        // 窓口の鍵が小さい位数の点になることは無いので、失敗するのは乱数の偶然だけ。作り直せば通る。
        let (secret, public) = link::derive_secret(link_kid, &server_public, client_secret)
            .ok_or(LinkError::Random(getrandom::Error::UNEXPECTED))?;
        let request = link::format_request(link_kid, now, &public, previous.as_ref());
        let mut file = state.file.clone();
        file.pending = Some(PendingLink {
            link_kid,
            secret: URL_SAFE_NO_PAD.encode(secret),
            request: request.clone(),
            created_at: now,
        });
        self.write(&mut state, file)?;
        Ok(request)
    }

    /// 結びかけを捨てる。
    pub fn cancel_link(&self) -> std::io::Result<()> {
        let mut state = self.lock();
        let mut file = state.file.clone();
        file.pending = None;
        self.write(&mut state, file)
    }

    /// 返しのコードを確かめ、結びかけを結び付きにする。
    pub fn accept_code(&self, code: &str) -> Result<Plan, CodeError> {
        let now = now();
        let mut state = self.lock();
        let pending = state.file.pending.clone().ok_or(CodeError::NotStarted)?;
        let secret = decode_secret(&pending.secret).ok_or(CodeError::NotStarted)?;
        let grant = link::parse_grant(&secret, code).ok_or(CodeError::Invalid)?;
        if now + CLOCK_TOLERANCE < grant.issued_at {
            return Err(CodeError::ClockBehind);
        }
        if grant.expires_at <= now {
            return Err(CodeError::Invalid);
        }
        let email = state.file.binding.as_ref().and_then(|b| b.email.clone());
        let mut file = state.file.clone();
        file.pending = None;
        file.binding = Some(Binding {
            installation: link::installation_id(&secret),
            link_kid: pending.link_kid,
            secret: pending.secret,
            email,
            permission: Some(Permission::Code {
                value: code.to_string(),
            }),
            last_checked_at: Some(now),
            refusal: None,
        });
        file.clock_high_water = file.clock_high_water.max(now);
        self.write(&mut state, file)?;
        tracing::info!(plan = ?grant.plan, "linked with a code");
        Ok(grant.plan)
    }

    /// 確かめに行くべきなら、その中身 (→ docs/pro.md「確かめる」)。結びかけは画面の問い合わせで確かめるので含めない。
    pub fn due_check(&self) -> Option<CheckRequest> {
        let now = now();
        let state = self.lock();
        let binding = state.file.binding.as_ref()?;
        let last = binding.last_checked_at.unwrap_or(0);
        let due = match (active_plan(&state, now), state.verified) {
            (Some(_), Some(v)) => {
                now - last >= CHECK_INTERVAL
                    || (v.expires_at - now < CHECK_BEFORE_EXPIRY
                        && now - last >= CHECK_WITHOUT_PERMISSION)
            }
            _ => now - last >= CHECK_WITHOUT_PERMISSION,
        };
        due.then(|| check_request(binding)).flatten()
    }

    /// 結んであれば、確かめる中身 (「今すぐ確かめる」)。
    pub fn bound_check(&self) -> Option<CheckRequest> {
        check_request(self.lock().file.binding.as_ref()?)
    }

    /// 結びかけの確かめの中身 (窓口で結ばれたか)。
    pub fn pending_check(&self) -> Option<CheckRequest> {
        let state = self.lock();
        let pending = state.file.pending.as_ref()?;
        let secret = decode_secret(&pending.secret)?;
        let installation = link::installation_id(&secret);
        Some(CheckRequest {
            target: CheckTarget::Pending(installation.clone()),
            installation,
            auth: link::check_auth(&secret),
        })
    }

    /// 窓口の答えで手元を合わせる。確かめている間に結び付きが変わっていれば、何もしない。
    /// 結びかけの確かめで `unbound` なら、まだ結ばれていないだけなので、そのまま待つ (`Ok(false)`)。
    pub fn apply_check(
        &self,
        target: &CheckTarget,
        outcome: CheckOutcome,
    ) -> Result<bool, ApplyError> {
        let now = now();
        let mut state = self.lock();
        let mut file = state.file.clone();
        match target {
            CheckTarget::Bound(id) => {
                let Some(binding) = file.binding.as_mut().filter(|b| &b.installation == id) else {
                    return Ok(false);
                };
                binding.last_checked_at = Some(now);
                match outcome {
                    CheckOutcome::Ok { proof, email } => {
                        receive_proof(&proof, id, now)?;
                        binding.permission = Some(Permission::Proof { value: proof });
                        binding.email = Some(email);
                        binding.refusal = None;
                    }
                    CheckOutcome::NoPlan { email } => refuse(binding, email, Refusal::NoPlan),
                    CheckOutcome::OverLimit { email } => refuse(binding, email, Refusal::OverLimit),
                    CheckOutcome::Unbound => {
                        tracing::info!(
                            "the account server says this installation is not linked; running as Free"
                        );
                        file.binding = None;
                    }
                }
            }
            CheckTarget::Pending(id) => {
                let Some(pending) = file.pending.clone() else {
                    return Ok(false);
                };
                let Some(secret) = decode_secret(&pending.secret) else {
                    return Ok(false);
                };
                if &link::installation_id(&secret) != id {
                    return Ok(false);
                }
                let mut binding = Binding {
                    installation: id.clone(),
                    link_kid: pending.link_kid,
                    secret: pending.secret,
                    email: None,
                    permission: None,
                    last_checked_at: Some(now),
                    refusal: None,
                };
                match outcome {
                    CheckOutcome::Unbound => return Ok(false),
                    CheckOutcome::Ok { proof, email } => {
                        receive_proof(&proof, id, now)?;
                        binding.permission = Some(Permission::Proof { value: proof });
                        binding.email = Some(email);
                    }
                    CheckOutcome::NoPlan { email } => refuse(&mut binding, email, Refusal::NoPlan),
                    CheckOutcome::OverLimit { email } => {
                        refuse(&mut binding, email, Refusal::OverLimit)
                    }
                }
                file.pending = None;
                file.binding = Some(binding);
                tracing::info!("linked to the account");
            }
        }
        file.clock_high_water = file.clock_high_water.max(now);
        self.write(&mut state, file)?;
        Ok(true)
    }

    /// アカウントから外す。Free に戻し、窓口へ渡す外した証しを返す (結んでいなければ `None`)。
    pub fn unbind(&self) -> std::io::Result<Option<String>> {
        let mut state = self.lock();
        let release = state
            .file
            .binding
            .as_ref()
            .and_then(|b| decode_secret(&b.secret))
            .map(|secret| link::release_code(&secret));
        let mut file = state.file.clone();
        file.binding = None;
        file.pending = None;
        self.write(&mut state, file)?;
        tracing::info!("removed this installation from the account; running as Free");
        Ok(release)
    }

    /// 見た最も新しい時刻を、ファイルに残す頃か (要求のたびには書かない)。
    pub fn high_water_due(&self) -> bool {
        self.lock().high_water_due()
    }

    /// 見た最も新しい時刻が進んでいれば、ファイルに残す。ファイルを書くので `spawn_blocking` から呼ぶ。
    pub fn persist_high_water(&self) {
        let mut state = self.lock();
        if !state.high_water_due() {
            return;
        }
        let file = state.file.clone();
        if let Err(err) = self.write(&mut state, file) {
            tracing::warn!(%err, "failed to write the Pro file");
        }
    }

    /// `file` を書いてから手元の値を入れ替える。書けなければ手元も変えない。
    fn write(&self, state: &mut State, file: ProFile) -> std::io::Result<()> {
        if file.is_empty() {
            match std::fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        } else {
            let bytes = serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?;
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // 書きかけの `pro.json` を残さないよう、隣に書いてから置き換える。
            crate::config::replace_owner_only_file(
                &self.path.with_extension("json.tmp"),
                &self.path,
                &bytes,
                false,
            )?;
        }
        state.verified = file.binding.as_ref().and_then(verify_binding);
        state.persisted_high_water = file.clock_high_water;
        state.file = file;
        Ok(())
    }
}

fn refuse(binding: &mut Binding, email: String, refusal: Refusal) {
    tracing::info!(
        ?refusal,
        "the account server gave no Pro permission; running as Free"
    );
    binding.permission = None;
    binding.email = Some(email);
    binding.refusal = Some(refusal);
}

fn check_request(binding: &Binding) -> Option<CheckRequest> {
    let secret = decode_secret(&binding.secret)?;
    Some(CheckRequest {
        target: CheckTarget::Bound(binding.installation.clone()),
        installation: binding.installation.clone(),
        auth: link::check_auth(&secret),
    })
}

/// 見た最も新しい時刻を進める (手元だけ。ファイルへは `persist_high_water`)。
fn observe(state: &mut State, now: i64) {
    if !state.file.is_empty() && now > state.file.clock_high_water {
        state.file.clock_high_water = now;
    }
}

fn clock_behind(state: &State, now: i64) -> bool {
    now + CLOCK_TOLERANCE < state.file.clock_high_water
}

fn active_plan(state: &State, now: i64) -> Option<Plan> {
    let v = state.verified?;
    (now < v.expires_at && !clock_behind(state, now)).then_some(v.plan)
}

fn decode_secret(text: &str) -> Option<[u8; 32]> {
    URL_SAFE_NO_PAD.decode(text).ok()?.try_into().ok()
}

fn verify_binding(binding: &Binding) -> Option<Verified> {
    let result = match binding.permission.as_ref()? {
        Permission::Proof { value } => {
            verify_proof(value, &binding.installation).map(|c| Verified {
                plan: c.plan,
                expires_at: c.expires_at,
                kind: PermissionKind::Online,
            })
        }
        Permission::Code { value } => decode_secret(&binding.secret)
            .and_then(|secret| link::parse_grant(&secret, value))
            .map(|g| Verified {
                plan: g.plan,
                expires_at: g.expires_at,
                kind: PermissionKind::Offline,
            })
            .ok_or(Error::Malformed),
    };
    result
        .inspect_err(|err| tracing::warn!(%err, "invalid Pro permission; running as Free"))
        .ok()
}

/// 証明に署名した鍵の公開鍵。`proof_kid` で引く。
type ProofKeys = &'static [(&'static str, [u8; 32])];
/// 結び付きの秘密を導く窓口の公開鍵。`link_kid` で引き、新しく結ぶときは最後のものを使う。
type LinkKeys = &'static [(u8, [u8; 32])];

/// 配る版が信じる証明の鍵。
/// - `prod-2026-09`: 本番の窓口 (`weblav.amiiby.com`) が署名する。秘密鍵は窓口の secret にだけあり、控えは無い (→ docs/pro.md「アカウントと販売の窓口」)。
#[cfg(not(debug_assertions))]
const PROOF_KEYS: ProofKeys = &[(
    "prod-2026-09",
    [
        0xea, 0x84, 0xf9, 0xed, 0x89, 0x5a, 0xd8, 0x15, 0xfa, 0xc3, 0x3f, 0x79, 0x87, 0x0f, 0x99,
        0x8c, 0x37, 0xc5, 0x54, 0x38, 0x85, 0x70, 0x89, 0x52, 0xf3, 0x1e, 0x54, 0x3b, 0xf1, 0xc4,
        0x05, 0x42,
    ],
)];

/// 配る版が使う結び付きの鍵。
/// - `1`: 本番の窓口 (`weblav.amiiby.com`) が導く。秘密鍵は窓口の secret にだけあり、控えは無い (→ docs/pro.md「アカウントと販売の窓口」)。
#[cfg(not(debug_assertions))]
const LINK_KEYS: LinkKeys = &[(
    1,
    [
        0x69, 0xf0, 0x0b, 0xbd, 0x0a, 0xcf, 0x83, 0x2c, 0x27, 0xec, 0xd8, 0x15, 0xdc, 0x3c, 0x50,
        0x32, 0xb1, 0x4b, 0xd2, 0xde, 0xdf, 0xcf, 0x23, 0x09, 0x15, 0x83, 0x96, 0xa0, 0x0f, 0x94,
        0xa4, 0x46,
    ],
)];

/// ADR: 開発版だけが信じる鍵 (`dev-local`)。手元で動かす窓口 (`just dev-account-server`) が使い、
/// テストと e2e の証明 (`tests/fixtures/dev-pro.json`) にも使う。秘密鍵は `account-server/dev.vars.example` にある。
#[cfg(debug_assertions)]
const PROOF_KEYS: ProofKeys = &[(
    "dev-local",
    [
        0x73, 0xf3, 0x7d, 0x92, 0x8d, 0x9d, 0x16, 0x95, 0x76, 0x6f, 0xcc, 0x26, 0x2b, 0x93, 0x74,
        0x71, 0x8c, 0x3e, 0x56, 0xcc, 0x73, 0xb5, 0x9d, 0x61, 0x89, 0xe3, 0x28, 0x56, 0x47, 0xd9,
        0x92, 0x6d,
    ],
)];

#[cfg(debug_assertions)]
const LINK_KEYS: LinkKeys = &[(
    0,
    [
        0x01, 0xbf, 0xdd, 0xed, 0xa9, 0x86, 0x7a, 0x51, 0x36, 0xa2, 0xe0, 0xd7, 0xe5, 0xb6, 0x8e,
        0xdb, 0xb8, 0x27, 0xc0, 0x4d, 0x0f, 0x54, 0x0f, 0x07, 0xd7, 0xf9, 0x92, 0x13, 0x6b, 0xf9,
        0x2e, 0x7b,
    ],
)];

// 新しく結ぶときは最後の鍵を使うので、どの版にも1つは要る。
const _: () = assert!(!LINK_KEYS.is_empty());

/// 証明の中身。知らない項目は読み飛ばす (項目を足しても前の版が読めるように)。
#[derive(Debug, Deserialize)]
struct Claims {
    v: u32,
    proof_kid: String,
    installation: String,
    plan: Plan,
    issued_at: i64,
    expires_at: i64,
}

/// 窓口から受け取った証明を確かめる。発行日より前の時計では受け取らない (→ docs/pro.md「結び付きと許可」の「時計」)。
fn receive_proof(proof: &str, installation: &str, now: i64) -> Result<(), ApplyError> {
    let claims = verify_proof(proof, installation).map_err(ApplyError::Invalid)?;
    if now + CLOCK_TOLERANCE < claims.issued_at {
        return Err(ApplyError::ClockBehind);
    }
    Ok(())
}

/// `base64url(中身).base64url(署名)` を確かめる。署名は中身の JSON のバイト列に対するもの。
fn verify_proof(proof: &str, installation: &str) -> Result<Claims, Error> {
    verify_proof_with(proof, installation, PROOF_KEYS)
}

fn verify_proof_with(proof: &str, installation: &str, keys: ProofKeys) -> Result<Claims, Error> {
    let (payload, signature) = proof.trim().split_once('.').ok_or(Error::Malformed)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| Error::Malformed)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| Error::Malformed)?;
    let signature = Signature::from_slice(&signature).map_err(|_| Error::Malformed)?;
    let claims: Claims = serde_json::from_slice(&payload).map_err(|_| Error::Malformed)?;
    if claims.v != 2 {
        return Err(Error::UnsupportedVersion(claims.v));
    }
    let key = keys
        .iter()
        .find(|(kid, _)| *kid == claims.proof_kid)
        .ok_or_else(|| Error::UnknownKey(claims.proof_kid.clone()))?;
    let key = VerifyingKey::from_bytes(&key.1).map_err(|_| Error::BadSignature)?;
    key.verify_strict(&payload, &signature)
        .map_err(|_| Error::BadSignature)?;
    if claims.installation != installation {
        return Err(Error::OtherInstallation);
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 開発版の窓口の鍵で署名した、期限が2100年の証明 (→ `tests/fixtures/dev-pro.json`)。
    const DEV_PRO: &str = include_str!("../tests/fixtures/dev-pro.json");

    fn dev_pro_file() -> ProFile {
        serde_json::from_str(DEV_PRO).expect("開発用の pro.json を読めなかった")
    }

    fn dev_proof() -> String {
        let Some(Permission::Proof { value }) = dev_pro_file().binding.and_then(|b| b.permission)
        else {
            panic!("開発用の pro.json に証明が無い");
        };
        value
    }

    /// 結びかけの秘密 (窓口が返しのコードを作るのに使うもの)。
    fn pending_secret(pro: &Pro) -> [u8; 32] {
        decode_secret(
            &pro.lock()
                .file
                .pending
                .as_ref()
                .expect("結びかけが無い")
                .secret,
        )
        .expect("秘密を読めなかった")
    }

    #[test]
    fn dev_proof_verifies() {
        let file = dev_pro_file();
        let verified = verify_binding(file.binding.as_ref().expect("結び付きが無い"))
            .expect("開発用の証明を確かめられなかった");
        assert_eq!(verified.plan, Plan::Personal);
        assert_eq!(verified.kind, PermissionKind::Online);
    }

    /// 手元の窓口が使う鍵と、開発版が信じる公開鍵が食い違わない。
    #[test]
    fn dev_local_keys_match_account_server() {
        let vars = include_str!("../account-server/dev.vars.example");
        let jwk_x = |name: &str| -> Vec<u8> {
            let jwk = vars
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .expect("dev.vars.example に鍵が無い");
            let jwk: serde_json::Value =
                serde_json::from_str(jwk).expect("鍵を JSON として読めなかった");
            URL_SAFE_NO_PAD
                .decode(jwk["x"].as_str().expect("JWK に x が無い"))
                .expect("JWK の x を base64url として読めなかった")
        };
        assert_eq!(jwk_x("PROOF_SIGNING_KEY="), PROOF_KEYS[0].1);
        assert_eq!(jwk_x("LINK_KEY="), LINK_KEYS[0].1);
    }

    #[test]
    fn proof_for_another_installation_is_rejected() {
        let value = dev_proof();
        assert!(matches!(
            verify_proof(&value, "ffffffffffffffff"),
            Err(Error::OtherInstallation)
        ));
    }

    #[test]
    fn rejects_garbage_and_unknown_keys() {
        assert!(matches!(verify_proof("", "x"), Err(Error::Malformed)));
        assert!(matches!(
            verify_proof("abc.def", "x"),
            Err(Error::Malformed)
        ));
        let value = dev_proof();
        assert!(matches!(
            verify_proof_with(&value, "0000000000000000", &[]),
            Err(Error::UnknownKey(_))
        ));
    }

    /// 結びかけて、返しのコードを打ち込むと Pro になり、外すと Free に戻ってファイルも消える。
    #[test]
    fn link_with_a_code_then_unbind() {
        let dir = crate::test_support::TempDir::new("pro-link-code");
        let path = dir.path().join("pro.json");
        let pro = Pro::load(&path);
        assert_eq!(pro.edition(), Edition::Free);

        let request = pro.start_link().expect("申し込みを作れなかった");
        assert!(pro.snapshot().pending_request.is_some());
        // 窓口と同じ作り方で、返しのコードを作る。
        let secret = pending_secret(&pro);
        let code = link::format_grant(&secret, Plan::Organization, now(), now() + 30 * DAY);
        assert!(matches!(
            pro.accept_code("AAAAA-AAAAA-AAAAA"),
            Err(CodeError::Invalid)
        ));
        assert_eq!(
            pro.accept_code(&code).expect("コードを受け取れなかった"),
            Plan::Organization
        );
        assert_eq!(pro.edition(), Edition::Pro);
        assert!(!request.is_empty());
        // 起動し直しても Pro のまま。
        assert_eq!(Pro::load(&path).plan(), Some(Plan::Organization));

        let release = pro.unbind().expect("外せなかった");
        assert!(release.is_some_and(|r| r.len() == 23));
        assert_eq!(pro.edition(), Edition::Free);
        assert!(!path.exists());
    }

    /// 時計を戻すと、期限の中でも Free として動く。
    #[test]
    fn clock_going_back_turns_free() {
        let dir = crate::test_support::TempDir::new("pro-clock");
        let path = dir.path().join("pro.json");
        let mut file = dev_pro_file();
        file.clock_high_water = now() + 2 * DAY;
        std::fs::write(&path, serde_json::to_vec(&file).expect("書けなかった"))
            .expect("書けなかった");
        let pro = Pro::load(&path);
        assert_eq!(pro.edition(), Edition::Free);
        assert!(pro.snapshot().clock_behind);
    }

    /// 期限まで7日を切っても、確かめた直後は確かめに行かない。1時間たてば行く。
    #[test]
    fn near_expiry_checks_at_most_hourly() {
        let dir = crate::test_support::TempDir::new("pro-near-expiry");
        let pro = Pro::load(&dir.path().join("pro.json"));
        pro.start_link().expect("申し込みを作れなかった");
        let secret = pending_secret(&pro);
        let code = link::format_grant(&secret, Plan::Personal, now(), now() + 3 * DAY);
        pro.accept_code(&code).expect("コードを受け取れなかった");
        assert!(pro.due_check().is_none());

        pro.lock()
            .file
            .binding
            .as_mut()
            .expect("結び付きが無い")
            .last_checked_at = Some(now() - CHECK_WITHOUT_PERMISSION);
        assert!(pro.due_check().is_some());
    }

    /// 窓口から受け取った証明でも、発行日より前の時計では受け取らない。
    #[test]
    fn proof_from_the_future_is_rejected() {
        let value = dev_proof();
        let issued_at = verify_proof(&value, "0000000000000000")
            .expect("開発用の証明を確かめられなかった")
            .issued_at;
        assert!(receive_proof(&value, "0000000000000000", issued_at - CLOCK_TOLERANCE).is_ok());
        assert!(matches!(
            receive_proof(&value, "0000000000000000", issued_at - CLOCK_TOLERANCE - 1),
            Err(ApplyError::ClockBehind)
        ));
    }
}
