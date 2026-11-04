//! フォルダーで見せるファイルの絞り込み (→ docs/folders.md「公開と非公開」)。
//!
//! 対象と除外のパターンを1行に1つ書く。除外が勝ち、対象が無ければ全ファイルが対象。
//! 順番で結果が変わらないように、`!` で混ぜる形にはしていない。
//! パスは登録先からの相対で、'/' で区切る。

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use super::error_detail::ValidationDetail;
use crate::error::AppError;

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

fn lines(raw: Option<&str>) -> impl Iterator<Item = &str> {
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
    fn normalize_trims_lines_and_drops_empty_ones() {
        assert_eq!(
            normalize_patterns(Some("  *.pdf \n\n a/ ")),
            Some("*.pdf\na/".to_string())
        );
        assert_eq!(normalize_patterns(Some(" \n ")), None);
        assert_eq!(normalize_patterns(None), None);
    }
}
