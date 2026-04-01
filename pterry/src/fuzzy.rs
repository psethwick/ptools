/// Returns `true` when `query` fuzzy-matches `target` (case-insensitive).
///
/// The match succeeds if:
/// - `query` is empty, or
/// - `target` contains `query` as a substring, or
/// - every character of `query` appears in `target` in order (subsequence match).
pub fn fuzzy_match(query: &str, target: &str) -> bool {
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    if query_lower.is_empty() {
        return true;
    }

    if target_lower.contains(&query_lower) {
        return true;
    }

    let mut query_chars = query_lower.chars();
    let target_chars = target_lower.chars();

    if let Some(mut query_char) = query_chars.next() {
        for target_char in target_chars {
            if query_char == target_char {
                if let Some(next_query_char) = query_chars.next() {
                    query_char = next_query_char;
                } else {
                    return true;
                }
            }
        }
        false
    } else {
        true
    }
}

/// Returns a relevance score for `query` against `target` (higher = better match).
///
/// Scoring tiers:
/// - 1000: exact match
/// - 500: `target` starts with `query`
/// - 100: `target` contains `query` as substring
/// - 10: subsequence (fuzzy) match
/// - 0: no match
/// - 1: empty query (matches everything equally)
pub fn fuzzy_score(query: &str, target: &str) -> i32 {
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    if query_lower.is_empty() {
        return 1;
    }

    if target_lower == query_lower {
        return 1000;
    }

    if target_lower.starts_with(&query_lower) {
        return 500;
    }

    if target_lower.contains(&query_lower) {
        return 100;
    }

    if fuzzy_match(query, target) {
        return 10;
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- fuzzy_match ---

    #[test]
    fn empty_query_always_matches() {
        assert!(fuzzy_match("", "anything"));
        assert!(fuzzy_match("", ""));
    }

    #[test]
    fn substring_matches() {
        assert!(fuzzy_match("fox", "the quick brown fox"));
        assert!(fuzzy_match("FOX", "the quick brown fox")); // case-insensitive
    }

    #[test]
    fn subsequence_matches() {
        assert!(fuzzy_match("abc", "a_b_c"));
        assert!(fuzzy_match("tqbf", "the quick brown fox"));
    }

    #[test]
    fn no_match_returns_false() {
        assert!(!fuzzy_match("xyz", "the quick brown fox"));
        assert!(!fuzzy_match("zz", "z"));
    }

    #[test]
    fn exact_match() {
        assert!(fuzzy_match("hello", "hello"));
    }

    // --- fuzzy_score ---

    #[test]
    fn empty_query_scores_one() {
        assert_eq!(fuzzy_score("", "anything"), 1);
    }

    #[test]
    fn exact_match_scores_1000() {
        assert_eq!(fuzzy_score("hello", "hello"), 1000);
        assert_eq!(fuzzy_score("HELLO", "hello"), 1000);
    }

    #[test]
    fn prefix_scores_500() {
        assert_eq!(fuzzy_score("hel", "hello"), 500);
    }

    #[test]
    fn substring_scores_100() {
        assert_eq!(fuzzy_score("ell", "hello"), 100);
    }

    #[test]
    fn subsequence_scores_10() {
        assert_eq!(fuzzy_score("hlo", "hello"), 10);
    }

    #[test]
    fn no_match_scores_zero() {
        assert_eq!(fuzzy_score("xyz", "hello"), 0);
    }

    #[test]
    fn higher_tier_beats_lower_tier() {
        assert!(fuzzy_score("hel", "hello") > fuzzy_score("ell", "hello"));
        assert!(fuzzy_score("ell", "hello") > fuzzy_score("hlo", "hello"));
    }
}
