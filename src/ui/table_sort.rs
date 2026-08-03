//! 表格多列排序状态与通用比较函数。

use std::{cmp::Ordering, net::IpAddr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    fn reverse_if_needed(self, ordering: Ordering) -> Ordering {
        match self {
            Self::Ascending => ordering,
            Self::Descending => ordering.reverse(),
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::Ascending => "↑",
            Self::Descending => "↓",
        }
    }

    fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SortRule<F> {
    field: F,
    direction: SortDirection,
}

pub(in crate::ui) struct MultiSort<F> {
    rules: Vec<SortRule<F>>,
}

impl<F: Copy + Eq> Default for MultiSort<F> {
    fn default() -> Self {
        Self { rules: Vec::new() }
    }
}

impl<F: Copy + Eq> MultiSort<F> {
    pub(in crate::ui) fn toggle(&mut self, field: F, additive: bool) {
        if let Some(rule) = self.rules.iter_mut().find(|rule| rule.field == field) {
            rule.direction = rule.direction.toggled();
            return;
        }

        if !additive {
            self.rules.clear();
        }
        self.rules.push(SortRule {
            field,
            direction: SortDirection::Ascending,
        });
    }

    pub(in crate::ui) fn label(&self, field: F, name: &str) -> String {
        self.rules
            .iter()
            .position(|rule| rule.field == field)
            .map(|index| {
                let rule = self.rules[index];
                format!("{name} {}{}", rule.direction.symbol(), index + 1)
            })
            .unwrap_or_else(|| name.to_string())
    }

    pub(in crate::ui) fn compare<T>(
        &self,
        left: &T,
        right: &T,
        compare_field: impl Fn(F, &T, &T) -> Ordering,
    ) -> Ordering {
        for rule in &self.rules {
            let ordering = rule
                .direction
                .reverse_if_needed(compare_field(rule.field, left, right));
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    }
}

pub(in crate::ui) fn compare_text(left: &str, right: &str) -> Ordering {
    left.to_lowercase().cmp(&right.to_lowercase())
}

pub(in crate::ui) fn compare_address(left: &str, right: &str) -> Ordering {
    match (left.parse::<IpAddr>(), right.parse::<IpAddr>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => compare_text(left, right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Field {
        Name,
        Address,
        Port,
    }

    #[test]
    fn supports_primary_and_additive_sort_rules() {
        let mut sort = MultiSort::default();
        sort.toggle(Field::Name, false);
        assert_eq!(sort.label(Field::Name, "名称"), "名称 ↑1");

        sort.toggle(Field::Port, true);
        assert_eq!(sort.label(Field::Name, "名称"), "名称 ↑1");
        assert_eq!(sort.label(Field::Port, "端口"), "端口 ↑2");

        sort.toggle(Field::Name, true);
        assert_eq!(sort.label(Field::Name, "名称"), "名称 ↓1");

        sort.toggle(Field::Port, false);
        assert_eq!(sort.label(Field::Name, "名称"), "名称 ↓1");
        assert_eq!(sort.label(Field::Port, "端口"), "端口 ↓2");
    }

    #[test]
    fn compares_ip_addresses_numerically() {
        assert_eq!(compare_address("10.0.0.2", "10.0.0.10"), Ordering::Less);
    }

    #[test]
    fn applies_sort_rules_in_priority_order() {
        let mut sort = MultiSort::default();
        sort.toggle(Field::Name, false);
        sort.toggle(Field::Port, true);
        let mut rows = vec![("beta", 1_u16), ("alpha", 2), ("alpha", 1)];
        rows.sort_by(|left, right| {
            sort.compare(left, right, |field, left, right| match field {
                Field::Name => compare_text(left.0, right.0),
                Field::Port => left.1.cmp(&right.1),
                Field::Address => Ordering::Equal,
            })
        });
        assert_eq!(rows, vec![("alpha", 1), ("alpha", 2), ("beta", 1)]);
    }

    #[test]
    fn toggles_a_secondary_field_without_resetting_its_priority() {
        let mut sort = MultiSort::default();
        sort.toggle(Field::Name, false);
        sort.toggle(Field::Address, true);

        let mut rows = vec![("aaaa", "10.123.1.23"), ("aaaa", "10.123.1.24")];
        rows.sort_by(|left, right| {
            sort.compare(left, right, |field, left, right| match field {
                Field::Name => compare_text(left.0, right.0),
                Field::Address => compare_address(left.1, right.1),
                Field::Port => Ordering::Equal,
            })
        });
        assert_eq!(rows[0].1, "10.123.1.23");

        sort.toggle(Field::Address, false);
        rows.sort_by(|left, right| {
            sort.compare(left, right, |field, left, right| match field {
                Field::Name => compare_text(left.0, right.0),
                Field::Address => compare_address(left.1, right.1),
                Field::Port => Ordering::Equal,
            })
        });
        assert_eq!(sort.label(Field::Name, "名称"), "名称 ↑1");
        assert_eq!(sort.label(Field::Address, "SSH 地址"), "SSH 地址 ↓2");
        assert_eq!(rows[0].1, "10.123.1.24");
    }
}
