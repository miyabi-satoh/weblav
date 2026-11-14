//! フォルダーで見せるものの絞り込み (→ docs/folders.md「見せるものの絞り込み」)。
//!
//! 対象と除外のパターンを1行に1つ書く。除外が勝ち、対象が無ければ全ファイルが対象。
//! 順番で結果が変わらないように、`!` で混ぜる形にはしていない。
//! パスは登録先からの相対で、'/' で区切り、実体の綴りで照らす
//! (一覧は `read_dir` の名前、配信は canonicalize した実体のパスから求める)。

use axum::Json;
use axum::extract::{Path, State};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contents::{self, ContentType, Visibility};
use super::error_detail::ValidationDetail;
use super::fs::{COUNT_LIMIT, walk_entries};
use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

/// コンパイル済みの絞り込み。
#[derive(Clone, Default)]
pub(super) struct FileFilter {
    /// 対象のファイル。パターンが無ければ `None` で、全ファイルが対象。
    include: Option<GlobSet>,
    /// ファイルにもフォルダーにも当てる除外。
    exclude: GlobSet,
    /// `/` で終わるパターンの除外。フォルダーにだけ当てる。
    exclude_dirs: GlobSet,
}

impl FileFilter {
    /// コンテンツの今の絞り込みを読む。パターンは数行なので、要求のたびに組み立てても軽く、
    /// メモリに持って古くなる心配をしなくて済む。
    pub(super) async fn load(pool: &SqlitePool, content_id: i64) -> Result<Self, AppError> {
        let patterns = sqlx::query!(
            "SELECT include_patterns, exclude_patterns FROM contents WHERE id = ?",
            content_id
        )
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
        Self::parse(
            patterns.include_patterns.as_deref(),
            patterns.exclude_patterns.as_deref(),
        )
    }

    /// 対象と除外の文字列 (1行に1パターン) から作る。正しくないパターンは 422 で、どの行かを返す。
    pub(super) fn parse(include: Option<&str>, exclude: Option<&str>) -> Result<Self, AppError> {
        let mut include_set = GlobSetBuilder::new();
        let mut has_include = false;
        for pattern in lines(include) {
            let parsed = Parsed::new(pattern);
            // フォルダーを対象にしたときは、その中のファイルを対象にする。対象はファイルにしか当てないため。
            let glob = if parsed.dir_only {
                format!("{}/**", parsed.glob)
            } else {
                parsed.glob
            };
            include_set.add(compile(&glob, pattern)?);
            has_include = true;
        }
        let mut exclude_set = GlobSetBuilder::new();
        let mut exclude_dirs = GlobSetBuilder::new();
        for pattern in lines(exclude) {
            let parsed = Parsed::new(pattern);
            let set = if parsed.dir_only {
                &mut exclude_dirs
            } else {
                &mut exclude_set
            };
            set.add(compile(&parsed.glob, pattern)?);
        }
        Ok(Self {
            include: if has_include {
                Some(build(include_set)?)
            } else {
                None
            },
            exclude: build(exclude_set)?,
            exclude_dirs: build(exclude_dirs)?,
        })
    }

    /// 何も外さないか (対象も除外も無い)。
    pub(super) fn is_empty(&self) -> bool {
        self.include.is_none() && self.exclude.is_empty() && self.exclude_dirs.is_empty()
    }

    /// フォルダー `rel` が除外されていないか。除外したフォルダーの中のフォルダーも除外する。
    pub(super) fn allows_dir(&self, rel: &str) -> bool {
        ancestors_and_self(rel)
            .all(|dir| !self.exclude.is_match(dir) && !self.exclude_dirs.is_match(dir))
    }

    /// ファイル `rel` が対象か。
    pub(super) fn allows_file(&self, rel: &str) -> bool {
        if let Some((parent, _)) = rel.rsplit_once('/')
            && !self.allows_dir(parent)
        {
            return false;
        }
        !self.exclude.is_match(rel)
            && self
                .include
                .as_ref()
                .is_none_or(|include| include.is_match(rel))
    }
}

/// 保存する形に揃える。前後の空白と空の行を除く。何も無ければ `None`。
pub(super) fn normalize_patterns(raw: Option<&str>) -> Option<String> {
    let joined = lines(raw).collect::<Vec<_>>().join("\n");
    (!joined.is_empty()).then_some(joined)
}

pub(super) fn lines(raw: Option<&str>) -> impl Iterator<Item = &str> {
    raw.unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

struct Parsed {
    glob: String,
    dir_only: bool,
}

impl Parsed {
    /// `/` を含まないパターンはどの階層の名前にも当て、含むものは登録先の一番上から見る (.gitignore と同じ)。
    fn new(pattern: &str) -> Self {
        let (body, dir_only) = match pattern.strip_suffix('/') {
            Some(body) => (body, true),
            None => (pattern, false),
        };
        let glob = match body.strip_prefix('/') {
            Some(anchored) => anchored.to_string(),
            None if body.contains('/') => body.to_string(),
            None => format!("**/{body}"),
        };
        Self { glob, dir_only }
    }
}

fn compile(glob: &str, pattern: &str) -> Result<globset::Glob, AppError> {
    GlobBuilder::new(glob)
        // 拡張子や名前の大文字小文字は、利用者が意識して使い分けるものではないため。
        .case_insensitive(true)
        // `*` が階層をまたぐと、`docs/*.pdf` が下の階層にも当たる。
        .literal_separator(true)
        .build()
        .map_err(|_| invalid(pattern))
}

fn build(builder: GlobSetBuilder) -> Result<GlobSet, AppError> {
    builder.build().map_err(|_| invalid(""))
}

fn invalid(pattern: &str) -> AppError {
    AppError::ValidationDetailed {
        message: format!("invalid file pattern: {pattern}"),
        detail: ValidationDetail::InvalidFilePattern {
            pattern: pattern.to_string(),
        },
    }
}

/// `a/b/c` なら `a`・`a/b`・`a/b/c`。
fn ancestors_and_self(rel: &str) -> impl Iterator<Item = &str> {
    rel.match_indices('/')
        .map(|(index, _)| &rel[..index])
        .chain(std::iter::once(rel))
        .filter(|dir| !dir.is_empty())
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct VisibilityPreviewRequest {
    include_patterns: Option<String>,
    exclude_patterns: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct VisibilityPreview {
    /// 今は見えず、保存すると見えるようになるファイルの数。
    gained: usize,
    /// 今は見えて、保存すると見えなくなるファイルの数。
    lost: usize,
    /// ファイルが多く、一部だけを数えたか。
    truncated: bool,
}

/// 見えるファイルの増減を数える。`files` は登録先からの相対パス。
fn count_visibility_changes<'a>(
    files: impl Iterator<Item = &'a str>,
    before: &FileFilter,
    after: &FileFilter,
) -> (usize, usize) {
    let (mut gained, mut lost) = (0, 0);
    for rel in files {
        match (before.allows_file(rel), after.allows_file(rel)) {
            (false, true) => gained += 1,
            (true, false) => lost += 1,
            _ => {}
        }
    }
    (gained, lost)
}

/// パターンを保存すると、見えるファイルがどう変わるかを数える。保存の前に知らせるため。
#[utoipa::path(
    post,
    path = "/contents/{id}/visibility-preview",
    params(("id" = i64, Path)),
    request_body = VisibilityPreviewRequest,
    responses(
        (status = OK, body = VisibilityPreview),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "書き換えられるのは管理者と作成者だけ"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、またはフォルダーでない"),
        (status = 422, body = crate::error::ErrorResponse, description = "パターンが正しくない"),
        (status = 503, body = crate::error::ErrorResponse, description = "フォルダーのある NAS やディスクにつながらない"),
    )
)]
async fn preview_visibility(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<VisibilityPreviewRequest>,
) -> Result<Json<VisibilityPreview>, AppError> {
    let row = sqlx::query!(
        r#"SELECT path, visibility as "visibility: Visibility", created_by,
                  list_view as "list_view: bool", last_scanned_at
           FROM contents WHERE id = ? AND type = ?"#,
        id,
        ContentType::Folder
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    contents::ensure_editable(&user, row.visibility, row.created_by)?;
    let root = contents::folder_path(row.path)?;
    let before = FileFilter::load(&state.pool, id).await?;
    let after = FileFilter::parse(
        payload.include_patterns.as_deref(),
        payload.exclude_patterns.as_deref(),
    )?;
    // ライブラリは、スキャンを終えていれば索引から数える。全ファイルが入っているのでディスクを読まず、打ち切らない。
    // 終えていない (中止した・場所を変えた) 索引は欠けているので、ディスクを読む。
    if row.list_view && row.last_scanned_at.is_some() {
        let rel_paths = sqlx::query_scalar!(
            "SELECT rel_path FROM archive_items WHERE archive_id = ?",
            id
        )
        .fetch_all(&state.pool)
        .await?;
        let preview = run_blocking(move || {
            let files = rel_paths.iter().map(String::as_str);
            let (gained, lost) = count_visibility_changes(files, &before, &after);
            VisibilityPreview {
                gained,
                lost,
                truncated: false,
            }
        })
        .await?;
        return Ok(Json(preview));
    }
    super::roots::ensure_reachable(&state, &root).await?;
    let preview = super::hidden::with_hidden(&state, move |hidden| {
        let root = std::fs::canonicalize(&root).map_err(|_| AppError::NotFound)?;
        let (entries, truncated) = walk_entries(&root, COUNT_LIMIT, hidden);
        let files = entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| entry.rel_path.as_str());
        let (gained, lost) = count_visibility_changes(files, &before, &after);
        Ok::<_, AppError>(VisibilityPreview {
            gained,
            lost,
            truncated,
        })
    })
    .await?;
    Ok(Json(preview))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(preview_visibility))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(include: &str, exclude: &str) -> FileFilter {
        FileFilter::parse(Some(include), Some(exclude)).expect("正しいパターンのはず")
    }

    #[test]
    fn empty_filter_allows_everything() {
        let filter = filter("", "");
        assert!(filter.allows_file("a/b.txt"));
        assert!(filter.allows_dir("a"));
    }

    #[test]
    fn pattern_without_slash_matches_a_name_at_any_depth() {
        let filter = filter("*.PDF", "");
        assert!(filter.allows_file("a.pdf"));
        assert!(filter.allows_file("x/y/a.pdf"));
        assert!(!filter.allows_file("a.txt"));
    }

    #[test]
    fn pattern_with_slash_starts_at_the_folder_root() {
        let filter = filter("docs/*.pdf", "");
        assert!(filter.allows_file("docs/a.pdf"));
        assert!(!filter.allows_file("docs/sub/a.pdf"));
        assert!(!filter.allows_file("x/docs/a.pdf"));
    }

    #[test]
    fn including_a_folder_includes_what_is_inside() {
        let filter = filter("docs/", "");
        assert!(filter.allows_file("x/docs/sub/a.pdf"));
        assert!(!filter.allows_file("x/a.pdf"));
    }

    #[test]
    fn exclude_wins_over_include() {
        let filter = filter("*.pdf", "*_old.*");
        assert!(filter.allows_file("a.pdf"));
        assert!(!filter.allows_file("a_old.pdf"));
    }

    #[test]
    fn excluding_a_folder_hides_everything_inside() {
        let filter = filter("", "draft/\n/tmp/cache");
        assert!(!filter.allows_dir("x/draft"));
        assert!(!filter.allows_dir("x/draft/sub"));
        assert!(!filter.allows_file("x/draft/sub/a.pdf"));
        assert!(filter.allows_file("draft"), "ファイルには当てないはず");
        assert!(!filter.allows_file("tmp/cache/a.pdf"));
        assert!(filter.allows_file("x/tmp/cache/a.pdf"));
    }

    /// 「パターンに追加...」は、`/` を含む語の候補の頭に `**/` を付ける (`$lib/pattern-candidate.ts`)。
    /// `/` を含むパターンは登録先の一番上から見るので、付けないと下の階層のパスに当たらない。
    #[test]
    fn a_candidate_with_a_slash_matches_at_any_depth() {
        let filter = filter("", "**/*2024/eng*");
        assert!(!filter.allows_file("2024/english.pdf"));
        assert!(!filter.allows_file("x/y/2024/english.pdf"));
        assert!(filter.allows_file("x/2023/english.pdf"));
        assert!(filter.allows_file("x/2024/math.pdf"));
    }

    /// 「パターンに追加...」は、絞り込みの語の中の記号を `[ ]` で包んで候補にする (`$lib/pattern-candidate.ts`)。
    /// ファイル名に記号があっても、その文字として当たること。
    #[test]
    fn symbols_wrapped_in_brackets_match_literally() {
        let filter = filter("", "*[[]mix[]]*\n*[{]a,b[}]*\n*[*]*\n*[?]*\n*[\\]*");
        // Windows のファイル名には `\` が入らず、globset も `\` を区切りの `/` に直して照らすため、Unix だけで確かめる。
        #[cfg(unix)]
        assert!(!filter.allows_file("back\\slash.mp3"));
        assert!(!filter.allows_file("x/song[mix].mp3"));
        assert!(!filter.allows_file("{a,b}.mp3"));
        assert!(!filter.allows_file("a*b.mp3"));
        assert!(!filter.allows_file("what?.mp3"));
        assert!(filter.allows_file("songmix.mp3"));
        assert!(filter.allows_file("a.mp3"));
    }

    #[test]
    fn invalid_pattern_reports_the_line() {
        let Err(AppError::ValidationDetailed { detail, .. }) =
            FileFilter::parse(Some("*.pdf\n[a"), None)
        else {
            panic!("正しくないパターンは断るはず");
        };
        assert!(matches!(
            detail,
            ValidationDetail::InvalidFilePattern { pattern } if pattern == "[a"
        ));
    }

    #[test]
    fn visibility_changes_count_files_in_both_directions() {
        let before = filter("*.pdf", "");
        let after = filter("", "secret.*");
        let files = ["a.pdf", "b.txt", "secret.pdf", "secret.txt"];
        // b.txt は対象を外したので増え、secret.pdf は除外したので減る。secret.txt はどちらでも見えない。
        assert_eq!(
            count_visibility_changes(files.into_iter(), &before, &after),
            (1, 1)
        );
        assert_eq!(
            count_visibility_changes(files.into_iter(), &after, &before),
            (1, 1)
        );
    }

    #[test]
    fn normalize_trims_lines_and_drops_empty_ones() {
        assert_eq!(
            normalize_patterns(Some("  *.pdf \n\n a/ ")),
            Some("*.pdf\na/".to_string())
        );
        assert_eq!(normalize_patterns(Some(" \n ")), None);
        assert_eq!(normalize_patterns(None), None);
    }
}
