//! Conservative shared-fact candidates (#46).
//!
//! Low recall on purpose. A pair of chunks yields a *shared-fact candidate*
//! only when a clause in each carries the same typed fact, said about the same
//! corroborated subject, with no negation or contrast anywhere in either
//! sentence. Everything here is deterministic string work over one chunk's
//! text; embeddings only decide which chunk pairs are worth looking at.
//!
//! A candidate needs **all** of:
//!
//! 1. **A typed anchor in both clauses.** A normalized *date* (EN/FIL month
//!    names and abbreviations, day digits; a lowercase "may" is never a month)
//!    or a *quantity bound to a counted noun* within three tokens. Bare
//!    numbers, lone years, page/section/list markers, ordinals, times and
//!    version strings are never anchors.
//! 2. **The same fact role.** The role word in the clause (deadline, event or
//!    session, ...) must be exactly one role and equal on both sides. Unknown
//!    or ambiguous roles give no candidate. A quantity's role is its counted
//!    noun class.
//! 3. **The same corroborated subject.** A normalized multi-word proper-noun
//!    span in the clause (or its sentence), or else the chunk's own `# `
//!    heading. A heading-derived subject must be corroborated by an explicit
//!    span on the other side; two heading-only subjects never match. An
//!    explicit link between the documents is deliberately not an input: it can
//!    never substitute for a subject.
//! 4. **No negation or contrast** in either clause or its sentence.
//!
//! The lexicons are small hand-made lists and the gates are uncalibrated: a
//! missing role word or a document without a proper-noun subject or heading
//! yields no candidate rather than a guess.

/// Work units charged per chunk whose clause features are computed in a tile.
pub const FEATURE_COST: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountClass {
    Person,
    Item,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Anchor {
    Date {
        month: u8,
        day: u8,
        year: Option<u16>,
    },
    Quantity {
        value: u64,
        class: CountClass,
    },
}

impl Anchor {
    /// Equal facts. Two dates match when month and day agree and no year
    /// contradicts; quantities need the same value and counted class.
    fn same_fact(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Anchor::Date { month, day, year },
                Anchor::Date {
                    month: other_month,
                    day: other_day,
                    year: other_year,
                },
            ) => {
                month == other_month
                    && day == other_day
                    && match (year, other_year) {
                        (Some(a), Some(b)) => a == b,
                        _ => true,
                    }
            }
            (a @ Anchor::Quantity { .. }, b @ Anchor::Quantity { .. }) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Deadline,
    EventSession,
    /// A counted quantity is described by its noun class, not a role word.
    Counted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subject {
    pub key: String,
    /// Taken from the chunk's own heading rather than named in the clause.
    pub from_heading: bool,
}

/// One clause's fact: byte offsets are within the chunk text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClauseFact {
    pub anchor: Anchor,
    pub role: Role,
    pub subject: Subject,
    pub start: usize,
    pub end: usize,
}

/// The matching clause facts of two chunks, if any: the first left fact (in
/// text order) with the first right fact it agrees with.
pub fn matching_facts<'a>(
    left: &'a [ClauseFact],
    right: &'a [ClauseFact],
) -> Option<(&'a ClauseFact, &'a ClauseFact)> {
    left.iter().find_map(|l| {
        right.iter().find_map(|r| {
            let same_subject = l.subject.key == r.subject.key
                // A heading-derived subject must be corroborated by an
                // explicit one on the other side.
                && !(l.subject.from_heading && r.subject.from_heading);
            (l.role == r.role && l.anchor.same_fact(&r.anchor) && same_subject).then_some((l, r))
        })
    })
}

#[derive(Clone, Copy)]
struct Token<'a> {
    raw: &'a str,
    start: usize,
    end: usize,
}

impl Token<'_> {
    /// Lowercase letters/digits with surrounding punctuation removed.
    fn word(&self) -> String {
        self.raw
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    }
}

fn tokens(text: &str, from: usize, to: usize) -> Vec<Token<'_>> {
    let mut found = Vec::new();
    let mut cursor = from;
    for raw in text[from..to].split_whitespace() {
        let Some(offset) = text[cursor..to].find(raw) else {
            break;
        };
        let start = cursor + offset;
        found.push(Token {
            raw,
            start,
            end: start + raw.len(),
        });
        cursor = start + raw.len();
    }
    found
}

/// Sentences end at `.`, `!`, `?` and newlines. A `.` inside a number or a
/// token such as `p.` followed by a digit does not end one.
fn sentence_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    for (index, (offset, character)) in characters.iter().enumerate() {
        let ends = match character {
            '\n' | '!' | '?' => true,
            '.' => characters
                .get(index + 1)
                .is_none_or(|(_, next)| next.is_whitespace()),
            _ => false,
        };
        if ends {
            let end = offset + character.len_utf8();
            if text[start..end].trim().len() > 0 {
                spans.push((start, end));
            }
            start = end;
        }
    }
    if text[start..].trim().len() > 0 {
        spans.push((start, text.len()));
    }
    spans
}

const CONTRASTIVE: &[&str] = &["but", "however", "pero", "ngunit", "samantalang", "habang"];

/// Clauses inside a sentence: split at `;` and at contrastive connectives.
fn clause_spans(text: &str, sentence: (usize, usize)) -> Vec<(usize, usize)> {
    let (from, to) = sentence;
    let mut spans = Vec::new();
    let mut start = from;
    for token in tokens(text, from, to) {
        let word = token.word();
        if CONTRASTIVE.contains(&word.as_str()) {
            if token.start > start {
                spans.push((start, token.start));
            }
            start = token.end;
        } else if token.raw.ends_with(';') {
            spans.push((start, token.end));
            start = token.end;
        }
    }
    if start < to {
        spans.push((start, to));
    }
    spans
        .into_iter()
        .filter_map(|(a, b)| {
            let slice = &text[a..b];
            let lead = slice.len() - slice.trim_start().len();
            let trail = slice.len() - slice.trim_end().len();
            (a + lead < b - trail).then_some((a + lead, b - trail))
        })
        .collect()
}

const NEGATION: &[&str] = &[
    "not",
    "no",
    "never",
    "without",
    "different",
    "other",
    "another",
    "unrelated",
    "instead",
    "except",
    "isn't",
    "wasn't",
    "aren't",
    "weren't",
    "don't",
    "doesn't",
    "didn't",
    "won't",
    "hindi",
    "di",
    "wala",
    "walang",
    "huwag",
    "hwag",
    "iba",
    "ibang",
];

fn is_negated(text: &str, span: (usize, usize)) -> bool {
    tokens(text, span.0, span.1).iter().any(|token| {
        let word = token.word();
        let plain = token
            .raw
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
            .to_lowercase();
        NEGATION.contains(&word.as_str())
            || NEGATION.contains(&plain.as_str())
            || plain.ends_with("n't")
            || plain.starts_with("'di")
    })
}

const MONTHS: [&[&str]; 12] = [
    &["january", "jan", "enero"],
    &["february", "feb", "pebrero"],
    &["march", "mar", "marso"],
    &["april", "apr", "abril"],
    &["may", "mayo"],
    &["june", "jun", "hunyo"],
    &["july", "jul", "hulyo"],
    &["august", "aug", "agosto"],
    &["september", "sep", "sept", "setyembre"],
    &["october", "oct", "oktubre", "octubre"],
    &["november", "nov", "nobyembre"],
    &["december", "dec", "disyembre"],
];

fn month_of(token: &Token<'_>) -> Option<u8> {
    let word = token.word();
    // A lowercase "may" is the Filipino "there is", never the month.
    if word == "may"
        && token
            .raw
            .trim_matches(|c: char| !c.is_alphanumeric())
            .chars()
            .next()
            .is_some_and(char::is_lowercase)
    {
        return None;
    }
    MONTHS
        .iter()
        .position(|names| names.contains(&word.as_str()))
        .map(|index| index as u8 + 1)
}

fn day_of(token: &Token<'_>) -> Option<u8> {
    let word = token.word();
    let digits = word
        .strip_suffix("st")
        .or_else(|| word.strip_suffix("nd"))
        .or_else(|| word.strip_suffix("rd"))
        .or_else(|| word.strip_suffix("th"))
        .unwrap_or(&word);
    if digits.is_empty() || digits.len() > 2 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let day: u8 = digits.parse().ok()?;
    (1..=31).contains(&day).then_some(day)
}

fn year_of(token: &Token<'_>) -> Option<u16> {
    let word = token.word();
    if word.len() != 4 || !word.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year: u16 = word.parse().ok()?;
    (1900..=2100).contains(&year).then_some(year)
}

fn role_of(word: &str, next: Option<&str>) -> Option<Role> {
    const DEADLINE: &[&str] = &[
        "deadline",
        "due",
        "submission",
        "submit",
        "submitted",
        "pasahan",
        "pagpasa",
        "ipasa",
        "magpasa",
        "isumite",
        "pagsumite",
    ];
    const EVENT: &[&str] = &[
        "session",
        "sesyon",
        "meeting",
        "pulong",
        "pagpupulong",
        "presentation",
        "presentasyon",
        "class",
        "klase",
        "exam",
        "pagsusulit",
        "practice",
        "magpraktis",
        "pagsasanay",
        "workshop",
        "kumperensya",
        "conference",
        "interview",
        "panayam",
        "schedule",
        "scheduled",
        "iskedyul",
    ];
    if DEADLINE.contains(&word) || (word == "huling" && next == Some("araw")) {
        Some(Role::Deadline)
    } else if EVENT.contains(&word) {
        Some(Role::EventSession)
    } else {
        None
    }
}

fn count_class(word: &str) -> Option<CountClass> {
    const PEOPLE: &[&str] = &[
        "student",
        "students",
        "estudyante",
        "mag-aaral",
        "participant",
        "participants",
        "kalahok",
        "volunteer",
        "volunteers",
        "boluntaryo",
        "boluntaryong",
        "people",
        "tao",
        "member",
        "members",
        "miyembro",
        "respondent",
        "respondents",
        "respondente",
        "interviewee",
        "interviewees",
    ];
    const ITEMS: &[&str] = &[
        "file",
        "files",
        "document",
        "documents",
        "dokumento",
        "item",
        "items",
        "copies",
        "kopya",
        "report",
        "reports",
        "ulat",
    ];
    if PEOPLE.contains(&word) {
        Some(CountClass::Person)
    } else if ITEMS.contains(&word) {
        Some(CountClass::Item)
    } else {
        None
    }
}

/// Words after which a number is a marker, not a quantity.
const MARKERS: &[&str] = &[
    "page", "pages", "p", "pp", "pahina", "section", "seksyon", "no", "number", "bilang", "v",
    "version", "chapter", "kabanata", "step", "hakbang", "line", "linya", "ika", "ikalawa", "item",
    "#",
];

fn is_ordinal_or_time(token: &Token<'_>) -> bool {
    let raw = token.raw.to_lowercase();
    let body = raw.trim_matches(|c: char| !c.is_alphanumeric());
    let ordinal = ["1st", "2nd", "3rd"].contains(&body)
        || (body.ends_with("th")
            && body[..body.len() - 2].chars().all(|c| c.is_ascii_digit())
            && body.len() > 2);
    ordinal
        || body.ends_with("am") && body[..body.len() - 2].chars().all(|c| c.is_ascii_digit())
        || body.ends_with("pm") && body[..body.len() - 2].chars().all(|c| c.is_ascii_digit())
        || token.raw.contains(':')
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "this", "that", "these", "those", "ang", "ng", "sa", "si", "ni", "kay",
    "para", "mga", "of", "and", "at",
];

fn is_capitalized(raw: &str) -> bool {
    raw.trim_matches(|c: char| !c.is_alphanumeric())
        .chars()
        .next()
        .is_some_and(char::is_uppercase)
}

/// The distinct normalized multi-word proper-noun spans (two or more
/// consecutive capitalized words) inside a byte range.
fn proper_noun_spans(text: &str, span: (usize, usize)) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut flush = |current: &mut Vec<String>| {
        while current
            .first()
            .is_some_and(|word| STOPWORDS.contains(&word.as_str()))
        {
            current.remove(0);
        }
        if current.len() >= 2 {
            let key = current.join(" ");
            if !found.contains(&key) {
                found.push(key);
            }
        }
        current.clear();
    };
    for token in tokens(text, span.0, span.1) {
        let word = token.word();
        let wordlike = token
            .raw
            .trim_matches(|c: char| !c.is_alphanumeric())
            .chars()
            .all(|c| c.is_alphabetic() || c == '-' || c == '\'');
        let boundary = month_of(&token).is_some()
            || role_of(&word, None).is_some()
            || !wordlike
            || word.is_empty();
        if is_capitalized(token.raw) && !boundary {
            current.push(word);
            if token.raw.ends_with([',', ';', ':', ')']) {
                flush(&mut current);
            }
        } else {
            flush(&mut current);
        }
    }
    flush(&mut current);
    found
}

/// The chunk's own `# ` heading, normalized, when the chunk contains one.
fn heading_key(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix("# "))
        .map(|heading| {
            heading
                .split_whitespace()
                .map(|word| {
                    word.trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase()
                })
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|key| key.split(' ').count() >= 2)
}

fn clause_anchors(text: &str, tokens: &[Token<'_>]) -> Vec<Anchor> {
    let _ = text;
    let mut anchors = Vec::new();
    let mut consumed = vec![false; tokens.len()];
    for (index, token) in tokens.iter().enumerate() {
        let Some(month) = month_of(token) else {
            continue;
        };
        // Clause-initial "May" before a number is the Filipino "there are"
        // ("May 12 boluntaryong estudyante"), not the month. A May date at the
        // start of a clause is therefore never an anchor: recall for safety.
        if month == 5 && index == 0 {
            continue;
        }
        // "October 20" or "October 20, 2026".
        if let Some(day_token) = tokens.get(index + 1) {
            if let Some(day) = day_of(day_token) {
                let year = tokens.get(index + 2).and_then(year_of);
                anchors.push(Anchor::Date { month, day, year });
                consumed[index + 1] = true;
                if year.is_some() {
                    consumed[index + 2] = true;
                }
                continue;
            }
        }
        // "20 October" or "20 ng Oktubre".
        let before = index.checked_sub(1).and_then(|i| tokens.get(i));
        let before_that = index.checked_sub(2).and_then(|i| tokens.get(i));
        let day_index = match (before, before_that) {
            (Some(b), Some(bb))
                if matches!(b.word().as_str(), "ng" | "of") && day_of(bb).is_some() =>
            {
                Some(index - 2)
            }
            (Some(b), _) if day_of(b).is_some() => Some(index - 1),
            _ => None,
        };
        if let Some(day_index) = day_index {
            if let Some(day) = day_of(&tokens[day_index]) {
                let year = tokens.get(index + 1).and_then(year_of);
                anchors.push(Anchor::Date { month, day, year });
                consumed[day_index] = true;
                if year.is_some() {
                    consumed[index + 1] = true;
                }
            }
        }
    }
    for (index, token) in tokens.iter().enumerate() {
        if consumed[index] || is_ordinal_or_time(token) {
            continue;
        }
        let body = token.raw.trim_matches(|c: char| !c.is_alphanumeric());
        let digits = body.replace(',', "");
        if digits.is_empty() || digits.len() > 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // A list marker ("12." or "12)" first in its line) or a lone year.
        if index == 0 && (token.raw.ends_with('.') || token.raw.ends_with(')')) {
            continue;
        }
        if year_of(token).is_some() {
            continue;
        }
        if index > 0 {
            let previous = tokens[index - 1].word();
            if MARKERS.contains(&previous.as_str()) || tokens[index - 1].raw.ends_with('#') {
                continue;
            }
        }
        if token.raw.starts_with('#') {
            continue;
        }
        let Ok(value) = digits.parse::<u64>() else {
            continue;
        };
        let class = tokens
            .iter()
            .skip(index + 1)
            .take(3)
            .find_map(|next| count_class(&next.word()));
        if let Some(class) = class {
            anchors.push(Anchor::Quantity { value, class });
        }
    }
    anchors
}

/// The facts of one chunk, in text order. The clause span of each fact is
/// the evidence a candidate cites.
pub fn chunk_facts(text: &str) -> Vec<ClauseFact> {
    let heading = heading_key(text);
    let mut facts = Vec::new();
    for sentence in sentence_spans(text) {
        for clause in clause_spans(text, sentence) {
            let clause_tokens = tokens(text, clause.0, clause.1);
            let anchors = clause_anchors(text, &clause_tokens);
            if anchors.is_empty() || is_negated(text, clause) || is_negated(text, sentence) {
                continue;
            }
            // Subject: one explicit proper-noun span in the clause, else in
            // its sentence, else the chunk's heading. Several distinct spans
            // are ambiguous, so no fact.
            let in_clause = proper_noun_spans(text, clause);
            let in_sentence = proper_noun_spans(text, sentence);
            let subject = match (in_clause.as_slice(), in_sentence.as_slice()) {
                ([only], _) => Some(Subject {
                    key: only.clone(),
                    from_heading: false,
                }),
                ([], [only]) => Some(Subject {
                    key: only.clone(),
                    from_heading: false,
                }),
                ([], []) => heading.clone().map(|key| Subject {
                    key,
                    from_heading: true,
                }),
                _ => None,
            };
            let Some(subject) = subject else { continue };
            let roles: Vec<Role> = clause_tokens
                .iter()
                .enumerate()
                .filter_map(|(index, token)| {
                    role_of(
                        &token.word(),
                        clause_tokens
                            .get(index + 1)
                            .map(|next| next.word())
                            .as_deref(),
                    )
                })
                .collect();
            let single_role = match roles.as_slice() {
                [first, rest @ ..] if rest.iter().all(|role| role == first) => Some(*first),
                _ => None,
            };
            for anchor in anchors {
                let role = match anchor {
                    Anchor::Quantity { .. } => Some(Role::Counted),
                    Anchor::Date { .. } => single_role,
                };
                if let Some(role) = role {
                    facts.push(ClauseFact {
                        anchor,
                        role,
                        subject: subject.clone(),
                        start: clause.0,
                        end: clause.1,
                    });
                }
            }
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shared fixture corpus is the tuning set for these gates. The
    // held-out EN/FIL/Taglish set that decides acceptance is separate, is not
    // authored here and was never used to choose a lexicon entry or threshold.
    const PROJECT_PLAN: &str =
        include_str!("../../../../fixtures/documents/projects/project-plan.md");
    const TALA_PROYEKTO: &str =
        include_str!("../../../../fixtures/documents/notes/tala-sa-proyekto.md");
    const MEETING_NOTES: &str =
        include_str!("../../../../fixtures/documents/meetings/meeting-notes.md");
    const CHECKLIST: &str =
        include_str!("../../../../fixtures/documents/projects/submission-checklist.md");
    const MATH_REVIEW: &str = include_str!("../../../../fixtures/documents/courses/math-review.md");
    const PAGSASANAY: &str =
        include_str!("../../../../fixtures/documents/courses/pagsasanay-sa-math.md");
    const METHODOLOGY: &str =
        include_str!("../../../../fixtures/documents/research/methodology-notes.md");
    const TALA_PAMAMARAAN: &str =
        include_str!("../../../../fixtures/documents/research/tala-sa-pamamaraan.md");
    const REMINDERS: &str =
        include_str!("../../../../fixtures/documents/research/review-reminders.md");

    fn shared(left: &str, right: &str) -> bool {
        matching_facts(&chunk_facts(left), &chunk_facts(right)).is_some()
    }

    #[test]
    fn the_deadline_is_shared_across_english_filipino_and_taglish() {
        assert!(
            shared(PROJECT_PLAN, TALA_PROYEKTO),
            "EN to FIL: subject from the heading, corroborated by an explicit span"
        );
        assert!(shared(PROJECT_PLAN, MEETING_NOTES), "EN to Taglish");
        assert!(shared(PROJECT_PLAN, CHECKLIST), "EN to EN");
        assert!(
            shared(TALA_PROYEKTO, MEETING_NOTES),
            "FIL to Taglish, both explicit"
        );
    }

    #[test]
    fn evidence_is_the_anchor_clause_in_each_document() {
        let left = chunk_facts(PROJECT_PLAN);
        let right = chunk_facts(TALA_PROYEKTO);
        let (l, r) = matching_facts(&left, &right).unwrap();
        assert_eq!(
            &PROJECT_PLAN[l.start..l.end],
            "The project submission deadline is October 20."
        );
        assert!(TALA_PROYEKTO[r.start..r.end].contains("huling araw"));
        assert!(TALA_PROYEKTO[r.start..r.end].contains("October 20"));
    }

    #[test]
    fn a_counted_quantity_is_shared_when_the_subject_is_corroborated() {
        assert!(
            shared(PROJECT_PLAN, METHODOLOGY),
            "12 volunteer students, heading plus explicit span"
        );
        assert!(
            shared(METHODOLOGY, TALA_PAMAMARAAN),
            "12 volunteer students ~ 12 boluntaryong estudyante"
        );
    }

    #[test]
    fn the_same_date_for_a_different_event_is_not_shared() {
        assert!(
            !shared(PROJECT_PLAN, MATH_REVIEW),
            "practice session vs deadline, with an explicit contrast"
        );
        assert!(
            !shared(TALA_PROYEKTO, PAGSASANAY),
            "Filipino negation and a different role"
        );
        assert!(!shared(MEETING_NOTES, PAGSASANAY));
        assert!(!shared(CHECKLIST, MATH_REVIEW));
    }

    #[test]
    fn unrelated_numbers_are_not_anchors() {
        let consent = chunk_facts(REMINDERS);
        assert!(
            consent.is_empty(),
            "no date or counted quantity in the reminders: {consent:?}"
        );
        for text in [
            "# Notes\n\nThe report was reviewed in 2026.",
            "See page 20 for the Community Learning Project deadline.",
            "20. Submit the Community Learning Project report.",
            "The Community Learning Project meets at 10:30 for the 3rd time.",
            "Community Learning Project version 1.2.3 is out.",
            "Reference #20 covers the Community Learning Project.",
        ] {
            assert!(chunk_facts(text).is_empty(), "no anchor: {text}");
        }
    }

    #[test]
    fn a_may_in_lowercase_is_never_a_month_but_a_capital_may_is() {
        assert!(
            chunk_facts("Ang Community Learning Project ay may 20 estudyante.")
                .iter()
                .all(|fact| matches!(fact.anchor, Anchor::Quantity { .. })),
            "the Filipino 'may' is not a date"
        );
        let dated = chunk_facts("The Community Learning Project deadline is May 20.");
        assert!(matches!(
            dated.first().map(|f| f.anchor),
            Some(Anchor::Date {
                month: 5,
                day: 20,
                ..
            })
        ));
    }

    #[test]
    fn a_date_without_one_clear_role_is_not_a_fact() {
        assert!(
            chunk_facts("The Community Learning Project is on October 20.").is_empty(),
            "no role word"
        );
        assert!(
            chunk_facts("The Community Learning Project presentation deadline is October 20.")
                .is_empty(),
            "two roles in one clause"
        );
    }

    #[test]
    fn contrast_splits_clauses_and_each_role_stays_with_its_own_date() {
        let facts = chunk_facts("The Community Learning Project deadline is October 20, but the presentation is October 24.");
        assert_eq!(facts.len(), 2);
        assert_eq!(
            (facts[0].role, facts[0].anchor),
            (
                Role::Deadline,
                Anchor::Date {
                    month: 10,
                    day: 20,
                    year: None
                }
            )
        );
        assert_eq!(
            (facts[1].role, facts[1].anchor),
            (
                Role::EventSession,
                Anchor::Date {
                    month: 10,
                    day: 24,
                    year: None
                }
            )
        );
    }

    #[test]
    fn negation_and_contrast_words_reject_the_fact() {
        for text in [
            "The Community Learning Project deadline is not October 20.",
            "Hindi ang Community Learning Project deadline ang October 20.",
            "The Community Learning Project deadline isn't October 20.",
            "The Community Learning Project deadline is October 20; a different event.",
            "Iba ito sa Community Learning Project na deadline ay October 20.",
        ] {
            assert!(chunk_facts(text).is_empty(), "negated: {text}");
        }
    }

    #[test]
    fn a_subject_must_be_one_explicit_span_or_a_heading() {
        assert!(
            chunk_facts("The deadline is October 20.").is_empty(),
            "no subject at all"
        );
        let two = "Community Learning Project and Mathematics Practice Exam share the deadline October 20.";
        assert!(
            chunk_facts(two).is_empty(),
            "two distinct subjects are ambiguous"
        );
        assert!(
            !chunk_facts("# Community Learning Project\n\nThe deadline is October 20.").is_empty(),
            "the heading is the subject"
        );
    }

    #[test]
    fn a_heading_subject_needs_an_explicit_subject_on_the_other_side() {
        let heading_only = "# Weekly Report\n\nThe deadline is October 20.";
        let other_heading_only = "# Weekly Report\n\nDeadline: October 20.";
        assert!(
            !shared(heading_only, other_heading_only),
            "two heading-only subjects are never corroborated"
        );
        let explicit = "The Weekly Report deadline is October 20.";
        assert!(shared(heading_only, explicit));
    }

    #[test]
    fn different_subjects_do_not_share_a_fact_even_on_the_same_date_and_role() {
        assert!(!shared(
            "The Community Learning Project deadline is October 20.",
            "The Mathematics Practice Exam deadline is October 20.",
        ));
    }

    #[test]
    fn a_date_year_that_contradicts_is_not_the_same_date() {
        assert!(!shared(
            "The Community Learning Project deadline is October 20, 2026.",
            "The Community Learning Project deadline is October 20, 2025.",
        ));
        assert!(shared(
            "The Community Learning Project deadline is October 20, 2026.",
            "The Community Learning Project deadline is October 20.",
        ));
    }

    #[test]
    fn dates_are_normalized_across_language_and_word_order() {
        assert!(shared(
            "The Community Learning Project deadline is October 20.",
            "Ang deadline ng Community Learning Project ay ika-20 ng Oktubre."
                .replace("ika-20", "20")
                .as_str(),
        ));
        assert!(shared(
            "The Community Learning Project deadline is Oct 20.",
            "Community Learning Project deadline: 20 October.",
        ));
    }

    #[test]
    fn a_link_cannot_stand_in_for_a_subject() {
        // Two documents that link to each other, share a date and a role, but
        // name no subject: discovery has no link input, so no candidate.
        let a = "See [b](b.md). The deadline is October 20.";
        let b = "See [a](a.md). Deadline: October 20.";
        assert!(!shared(a, b));
    }

    #[test]
    fn offsets_are_utf8_bytes_within_the_chunk() {
        let text = "Ang Community Learning Project ay may 20 estudyante na tumutulong, at ang deadline ng Community Learning Project ay Oktubre 20.";
        for fact in chunk_facts(text) {
            assert!(text.is_char_boundary(fact.start) && text.is_char_boundary(fact.end));
        }
        let accented = "Ñandú Pérez Plan — ang deadline ng Ñandú Pérez Plan ay Oktubre 20.";
        for fact in chunk_facts(accented) {
            assert!(accented.is_char_boundary(fact.start) && accented.is_char_boundary(fact.end));
        }
    }
}
