//! Search operators, in the style of Google Scholar. A query that uses none of
//! them keeps the plain keyword search; one that does is matched exactly:
//!
//! - `"1_b"`: the exact text, as a whole token. It doesn't match `1_c`, `1 b`,
//!   `2_b` or `11_b`. Case and accents don't matter.
//! - `-draft`, `-"old plan"`: files containing it are left out.
//! - `budget OR gastos`: either one (uppercase `OR`; `AND` is the default and
//!   may be written out).
//! - `intitle:resume`: in the file's name or title.
//! - `filetype:pdf` (or `ext:pdf`): by extension.
//! - `in:projects/2024` (or `folder:`): inside that folder, at any depth.
//!
//! The twin of `src/domain/searchQuery.ts`, which applies the same rules to
//! names before a folder is indexed. Change both together.

use unicode_normalization::UnicodeNormalization;

/// Enough for any query a person types; the rest is ignored.
const MAX_TERMS: usize = 32;

#[derive(Debug, Default, PartialEq)]
pub struct SearchQuery {
    /// Every clause must match; a clause matches when any of its terms does.
    clauses: Vec<Vec<String>>,
    excluded: Vec<String>,
    in_title: Vec<String>,
    filetypes: Vec<String>,
    folders: Vec<String>,
}

/// The parts of a file a query is matched against.
pub struct Searchable<'a> {
    pub name: &'a str,
    pub title: &'a str,
    pub relative_path: &'a str,
    pub text: &'a str,
}

#[derive(Debug)]
struct Token {
    negated: bool,
    field: Option<String>,
    value: String,
    quoted: bool,
}

/// Case- and accent-insensitive, with every run of whitespace as one space, so
/// a phrase matches across line breaks.
pub fn fold(value: &str) -> String {
    let lowered: String = value
        .nfd()
        .filter(|character| !unicode_normalization::char::is_combining_mark(*character))
        .collect::<String>()
        .to_lowercase();
    lowered.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Where `needle` (folded) occurs in `haystack` (folded) as a whole token: not
/// preceded or followed by a letter, digit or underscore.
fn occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(needle) {
        let start = from + offset;
        let end = start + needle.len();
        let before = haystack[..start].chars().next_back().is_none_or(|ch| !is_word(ch));
        let after = haystack[end..].chars().next().is_none_or(|ch| !is_word(ch));
        if before && after {
            count += 1;
        }
        from = start + haystack[start..].chars().next().map_or(1, char::len_utf8);
    }
    count
}

fn tokens(query: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut characters = query.chars().peekable();
    while let Some(&next) = characters.peek() {
        if next.is_whitespace() {
            characters.next();
            continue;
        }
        let negated = next == '-';
        if negated {
            characters.next();
        }
        let mut raw = String::new();
        let mut quoted = false;
        let mut field = None;
        loop {
            match characters.peek() {
                Some('"') => {
                    characters.next();
                    quoted = true;
                    for character in characters.by_ref() {
                        if character == '"' {
                            break;
                        }
                        raw.push(character);
                    }
                    break;
                }
                Some(&character) if !character.is_whitespace() => {
                    characters.next();
                    raw.push(character);
                    if character == ':' && field.is_none() {
                        let name = raw.trim_end_matches(':').to_lowercase();
                        if matches!(
                            name.as_str(),
                            "intitle" | "filetype" | "ext" | "in" | "folder"
                        ) {
                            field = Some(name);
                            raw.clear();
                        }
                    }
                }
                _ => break,
            }
        }
        if negated && raw.is_empty() && !quoted && field.is_none() {
            continue; // a lone "-"
        }
        tokens.push(Token {
            negated,
            field,
            value: raw,
            quoted,
        });
    }
    tokens
}

impl SearchQuery {
    /// `None` when the query uses no operator, so it keeps the plain search.
    pub fn parse(query: &str) -> Option<Self> {
        let tokens = tokens(query);
        let uses_operators = tokens.iter().any(|token| {
            token.negated
                || token.quoted
                || token.field.is_some()
                || (!token.quoted && (token.value == "OR" || token.value == "AND"))
        });
        if !uses_operators {
            return None;
        }
        let mut parsed = SearchQuery::default();
        let mut join_next = false;
        let mut terms = 0;
        for token in tokens {
            if !token.quoted && !token.negated && token.field.is_none() {
                if token.value == "OR" {
                    join_next = !parsed.clauses.is_empty();
                    continue;
                }
                if token.value == "AND" {
                    continue;
                }
            }
            let value = fold(&token.value);
            if value.is_empty() || terms >= MAX_TERMS {
                continue;
            }
            terms += 1;
            match token.field.as_deref() {
                Some("intitle") if !token.negated => parsed.in_title.push(value),
                Some("filetype") | Some("ext") if !token.negated => parsed
                    .filetypes
                    .push(value.trim_start_matches('.').to_string()),
                Some("in") | Some("folder") if !token.negated => parsed
                    .folders
                    .push(value.trim_matches('/').replace('\\', "/")),
                Some(_) => parsed.excluded.push(value),
                None if token.negated => parsed.excluded.push(value),
                None => {
                    if join_next {
                        parsed.clauses.last_mut().expect("joined").push(value);
                    } else {
                        parsed.clauses.push(vec![value]);
                    }
                }
            }
            join_next = false;
        }
        Some(parsed)
    }

    /// Words a match should be shown around.
    pub fn highlight_terms(&self) -> Vec<String> {
        self.clauses
            .iter()
            .flatten()
            .chain(&self.in_title)
            .cloned()
            .collect()
    }

    /// Whether the file matches, and how strongly: `None` when it doesn't.
    pub fn score(&self, file: &Searchable<'_>) -> Option<f64> {
        let extension = file
            .name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_lowercase())
            .unwrap_or_default();
        if !self.filetypes.is_empty() && !self.filetypes.contains(&extension) {
            return None;
        }
        if !self.folders.is_empty() {
            let folder = file
                .relative_path
                .rsplit_once('/')
                .map_or("", |(folder, _)| folder);
            let folder = format!("/{}/", fold(folder));
            if !self
                .folders
                .iter()
                .any(|wanted| folder.contains(&format!("/{wanted}/")))
            {
                return None;
            }
        }
        let title = fold(&format!("{} {}", file.name, file.title));
        // File names use `_` as a separator ("VILAR_Resume"), so a name is also
        // read with it as a space. A term with `_` in it still needs it.
        let title = format!("{title} {}", title.replace('_', " "));
        let body = fold(file.text);
        let mut score = 0.0;
        for wanted in &self.in_title {
            let found = occurrences(&title, wanted);
            if found == 0 {
                return None;
            }
            score += 5.0 + found as f64;
        }
        for clause in &self.clauses {
            let mut matched = false;
            for term in clause {
                let in_title = occurrences(&title, term);
                let in_body = occurrences(&body, term);
                if in_title + in_body > 0 {
                    matched = true;
                    score += 3.0 * in_title.min(1) as f64 + (in_body.min(20) as f64);
                }
            }
            if !matched {
                return None;
            }
        }
        if self
            .excluded
            .iter()
            .any(|term| occurrences(&title, term) + occurrences(&body, term) > 0)
        {
            return None;
        }
        Some(score)
    }

    /// Whether a passage of text contains any term the query asks for.
    pub fn mentions(&self, text: &str) -> bool {
        let text = fold(text);
        self.highlight_terms()
            .iter()
            .any(|term| occurrences(&text, term) > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(relative_path: &'a str, text: &'a str) -> Searchable<'a> {
        let name = relative_path.rsplit('/').next().unwrap();
        Searchable {
            name,
            title: name,
            relative_path,
            text,
        }
    }

    fn matches(query: &str, relative_path: &str, text: &str) -> bool {
        SearchQuery::parse(query)
            .expect("uses operators")
            .score(&file(relative_path, text))
            .is_some()
    }

    #[test]
    fn plain_queries_keep_the_plain_search() {
        assert_eq!(SearchQuery::parse("budget notes"), None);
        assert_eq!(SearchQuery::parse("budget or notes"), None);
        assert_eq!(SearchQuery::parse("pre-school"), None);
    }

    #[test]
    fn a_quoted_text_matches_only_that_exact_token() {
        assert!(matches("\"1_b\"", "a.md", "Section 1_b covers fees."));
        assert!(matches("\"1_b\"", "a.md", "SECTION 1_B"));
        for other in ["Section 1_c", "Section 1 b", "Section 2_b", "Section 11_b", "Section 1_bc", "x_1_b"] {
            assert!(!matches("\"1_b\"", "a.md", other), "{other}");
        }
    }

    #[test]
    fn a_quoted_text_also_matches_a_file_name() {
        assert!(matches("\"1_b\"", "forms/1_b.pdf", ""));
        assert!(!matches("\"1_b\"", "forms/1_c.pdf", ""));
    }

    #[test]
    fn a_phrase_matches_across_line_breaks_and_accents() {
        assert!(matches("\"project plan\"", "a.md", "the Project\n  plan is due"));
        assert!(matches("\"nino\"", "a.md", "si Niño"));
        assert!(!matches("\"project plan\"", "a.md", "the plan for the project"));
    }

    #[test]
    fn excluded_terms_leave_files_out() {
        assert!(matches("budget -draft", "a.md", "final budget"));
        assert!(!matches("budget -draft", "a.md", "draft budget"));
        assert!(!matches("budget -\"old plan\"", "a.md", "budget for the old plan"));
        assert!(!matches("budget -draft", "draft.md", "budget"));
    }

    #[test]
    fn uppercase_or_accepts_either_and_every_other_term_is_required() {
        assert!(matches("\"budget\" OR gastos", "a.md", "mga gastos"));
        assert!(matches("\"budget\" OR gastos", "a.md", "the budget"));
        assert!(!matches("\"budget\" OR gastos", "a.md", "nothing here"));
        assert!(!matches("\"budget\" AND gastos", "a.md", "the budget"));
        assert!(matches("\"budget\" AND gastos", "a.md", "budget at gastos"));
    }

    #[test]
    fn fields_filter_by_title_type_and_folder() {
        assert!(matches("intitle:resume", "career/VILAR_Resume.pdf", ""));
        assert!(!matches("intitle:resume", "career/cover.pdf", "my resume"));
        assert!(matches("filetype:pdf", "a/b.pdf", ""));
        assert!(!matches("ext:.pdf", "a/b.md", ""));
        assert!(matches("in:projects", "work/projects/2024/a.md", ""));
        assert!(matches("folder:work/projects", "work/projects/2024/a.md", ""));
        assert!(!matches("in:projects", "work/project/a.md", ""));
        assert!(!matches("in:projects", "projects.md", ""));
    }

    #[test]
    fn unknown_fields_and_colons_are_plain_text() {
        let query = SearchQuery::parse("\"x\" note:today").unwrap();
        assert_eq!(query.clauses, vec![vec!["x".to_string()], vec!["note:today".to_string()]]);
    }

    #[test]
    fn titles_rank_above_body_mentions() {
        let query = SearchQuery::parse("\"budget\"").unwrap();
        let named = query.score(&file("budget.md", "")).unwrap();
        let mentioned = query.score(&file("notes.md", "budget")).unwrap();
        assert!(named > mentioned);
    }
}
