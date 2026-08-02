//! 列表页和选择器共用的正则搜索逻辑。

use regex::{Regex, RegexBuilder};

pub(super) struct RegexSearch {
    regex: Option<Regex>,
    literal_fallback: String,
    error: Option<String>,
}

impl RegexSearch {
    pub(super) fn new(query: &str) -> Self {
        let query = query.trim();
        if query.is_empty() {
            return Self {
                regex: None,
                literal_fallback: String::new(),
                error: None,
            };
        }

        match RegexBuilder::new(query).case_insensitive(true).build() {
            Ok(regex) => Self {
                regex: Some(regex),
                literal_fallback: String::new(),
                error: None,
            },
            Err(error) => {
                let reason = error
                    .to_string()
                    .lines()
                    .last()
                    .unwrap_or("正则表达式不完整")
                    .trim()
                    .to_string();
                Self {
                    regex: None,
                    literal_fallback: query.to_lowercase(),
                    error: Some(format!("正则格式错误，当前按普通文本搜索：{reason}")),
                }
            }
        }
    }

    pub(super) fn matches(&self, value: &str) -> bool {
        if self.regex.is_none() && self.literal_fallback.is_empty() {
            return true;
        }
        self.regex.as_ref().map_or_else(
            || value.to_lowercase().contains(&self.literal_fallback),
            |regex| regex.is_match(value),
        )
    }

    pub(super) fn matches_any<'a>(&self, values: impl IntoIterator<Item = &'a str>) -> bool {
        values.into_iter().any(|value| self.matches(value))
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::RegexSearch;

    #[test]
    fn regex_search_supports_case_insensitive_patterns_and_anchors() {
        let search = RegexSearch::new(r"^(prod|stage)-\d+$");
        assert!(search.matches("PROD-01"));
        assert!(search.matches("stage-12"));
        assert!(!search.matches("test-prod-01"));
        assert!(search.error().is_none());
    }

    #[test]
    fn invalid_regex_falls_back_to_literal_search() {
        let search = RegexSearch::new("server[");
        assert!(search.matches("primary-server["));
        assert!(!search.matches("primary-server"));
        assert!(search.error().is_some());
    }

    #[test]
    fn empty_regex_matches_everything() {
        assert!(RegexSearch::new("  ").matches("anything"));
    }
}
