//! Versions and constraints.
//!
//! A BendHub package version is four numbers, major.minor.patch.build,
//! the same shape the hub's NAMED regex accepts.

use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version(pub u32, pub u32, pub u32, pub u32);

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let mut parts = [0u32; 4];
        let mut seen = 0;
        for (i, part) in text.split('.').enumerate() {
            if i >= 4 || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            if part.len() > 1 && part.starts_with('0') {
                return None;
            }
            parts[i] = part.parse().ok()?;
            seen += 1;
        }
        if seen == 0 {
            return None;
        }
        Some(Version(parts[0], parts[1], parts[2], parts[3]))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0, self.1, self.2, self.3)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .cmp(&other.0)
            .then(self.1.cmp(&other.1))
            .then(self.2.cmp(&other.2))
            .then(self.3.cmp(&other.3))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug)]
pub enum Term {
    Any,
    Eq(Version),
    Gt(Version),
    Ge(Version),
    Lt(Version),
    Le(Version),
}

#[derive(Clone, Debug)]
pub struct Constraint {
    pub terms: Vec<Term>,
    pub source: String,
}

impl Constraint {
    pub fn parse(text: &str) -> Result<Constraint, String> {
        let raw = text.trim();
        if raw.is_empty() || raw == "*" || raw.eq_ignore_ascii_case("latest") {
            return Ok(Constraint {
                terms: vec![Term::Any],
                source: raw.to_string(),
            });
        }
        let mut terms = Vec::new();
        for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            terms.extend(one(part)?);
        }
        if terms.is_empty() {
            terms.push(Term::Any);
        }
        Ok(Constraint {
            terms,
            source: raw.to_string(),
        })
    }

    pub fn matches(&self, v: &Version) -> bool {
        self.terms.iter().all(|t| match t {
            Term::Any => true,
            Term::Eq(x) => v == x,
            Term::Gt(x) => v > x,
            Term::Ge(x) => v >= x,
            Term::Lt(x) => v < x,
            Term::Le(x) => v <= x,
        })
    }

    /// exact says whether the constraint names one version and nothing else.
    pub fn exact(&self) -> Option<Version> {
        if let [Term::Eq(v)] = self.terms.as_slice() {
            return Some(*v);
        }
        None
    }
}

fn one(part: &str) -> Result<Vec<Term>, String> {
    if let Some(rest) = part.strip_prefix('^') {
        return caret(rest);
    }
    if let Some(rest) = part.strip_prefix('~') {
        return tilde(rest);
    }
    let (op, rest) = if let Some(r) = part.strip_prefix(">=") {
        ("ge", r)
    } else if let Some(r) = part.strip_prefix("<=") {
        ("le", r)
    } else if let Some(r) = part.strip_prefix('>') {
        ("gt", r)
    } else if let Some(r) = part.strip_prefix('<') {
        ("lt", r)
    } else if let Some(r) = part.strip_prefix('=') {
        ("eq", r)
    } else {
        ("eq", part)
    };
    let full = Version::parse(rest).ok_or_else(|| format!("a version constraint: {part}"))?;
    if op == "eq" && rest.split('.').count() < 4 {
        return caret(rest);
    }
    Ok(vec![match op {
        "eq" => Term::Eq(full),
        "gt" => Term::Gt(full),
        "ge" => Term::Ge(full),
        "lt" => Term::Lt(full),
        _ => Term::Le(full),
    }])
}

/// caret reads "^1.2.3" as at least 1.2.3.0 and below 2.0.0.0, and
/// "^0.2.3" as below 0.3.0.0: the first number that is not zero is the
/// one that may move. "^0.0.3" stops at 0.0.4.0.
fn caret(text: &str) -> Result<Vec<Term>, String> {
    let parts: Vec<&str> = text.split('.').collect();
    let base =
        Version::parse(&parts.join(".")).ok_or_else(|| format!("a version constraint: ^{text}"))?;
    let upper = if base.0 != 0 {
        Version(base.0 + 1, 0, 0, 0)
    } else if parts.len() == 1 {
        Version(1, 0, 0, 0)
    } else if base.1 != 0 {
        Version(0, base.1 + 1, 0, 0)
    } else if parts.len() == 2 {
        Version(0, 1, 0, 0)
    } else if base.2 != 0 {
        Version(0, 0, base.2 + 1, 0)
    } else {
        Version(0, 0, 0, base.3 + 1)
    };
    Ok(vec![Term::Ge(base), Term::Lt(upper)])
}

/// tilde reads "~1.2.3" as at least 1.2.3.0 and below 1.3.0.0: the
/// minor number moves, the major one does not. "~1" stops at 2.0.0.0.
fn tilde(text: &str) -> Result<Vec<Term>, String> {
    let parts: Vec<&str> = text.split('.').collect();
    let base =
        Version::parse(&parts.join(".")).ok_or_else(|| format!("a version constraint: ~{text}"))?;
    let upper = if parts.len() <= 1 {
        Version(base.0 + 1, 0, 0, 0)
    } else {
        Version(base.0, base.1 + 1, 0, 0)
    };
    Ok(vec![Term::Ge(base), Term::Lt(upper)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_four_numbers() {
        assert_eq!(v("1.2.3.4"), Version(1, 2, 3, 4));
    }

    #[test]
    fn pads_a_shorter_version() {
        assert_eq!(v("1.2"), Version(1, 2, 0, 0));
        assert_eq!(v("1.2.3"), Version(1, 2, 3, 0));
    }

    #[test]
    fn refuses_a_version_that_is_not_numbers() {
        assert!(Version::parse("1.2.x").is_none());
        assert!(Version::parse("").is_none());
        assert!(Version::parse("01.2.3.4").is_none());
    }

    #[test]
    fn orders_by_each_number_in_turn() {
        assert!(v("0.1.14.0") > v("0.1.13.9"));
        assert_eq!(v("1.0.0.0").cmp(&v("1.0.0.0")), Ordering::Equal);
        assert!(v("1.0.0.0") > v("0.9.9.9"));
    }

    #[test]
    fn any_takes_every_version() {
        for text in ["*", "latest", ""] {
            assert!(Constraint::parse(text).unwrap().matches(&v("9.9.9.9")));
        }
    }

    #[test]
    fn an_exact_version_takes_only_itself() {
        let c = Constraint::parse("0.1.14.0").unwrap();
        assert!(c.matches(&v("0.1.14.0")));
        assert!(!c.matches(&v("0.1.14.1")));
        assert!(!c.matches(&v("0.1.13.0")));
        assert_eq!(c.exact(), Some(v("0.1.14.0")));
    }

    #[test]
    fn a_caret_moves_the_major_or_the_minor_when_the_major_is_zero() {
        let c = Constraint::parse("^1.2.3").unwrap();
        assert!(c.matches(&v("1.2.3.0")));
        assert!(c.matches(&v("1.9.9.9")));
        assert!(!c.matches(&v("2.0.0.0")));
        assert!(!c.matches(&v("1.2.2.9")));

        let c = Constraint::parse("^0.1.14").unwrap();
        assert!(c.matches(&v("0.1.14.0")));
        assert!(c.matches(&v("0.1.99.0")));
        assert!(!c.matches(&v("0.2.0.0")));
    }

    #[test]
    fn a_caret_under_two_zeros_stops_at_the_patch() {
        let c = Constraint::parse("^0.0.3").unwrap();
        assert!(c.matches(&v("0.0.3.0")));
        assert!(!c.matches(&v("0.0.4.0")));
        assert!(!c.matches(&v("0.1.0.0")));
    }

    #[test]
    fn a_tilde_moves_the_minor_only() {
        let c = Constraint::parse("~1.2.3").unwrap();
        assert!(c.matches(&v("1.2.3.0")));
        assert!(c.matches(&v("1.2.99.0")));
        assert!(!c.matches(&v("1.3.0.0")));
        assert!(!c.matches(&v("2.0.0.0")));
    }

    #[test]
    fn a_bare_short_version_reads_as_a_caret() {
        let c = Constraint::parse("1.2").unwrap();
        assert!(c.matches(&v("1.2.0.0")));
        assert!(c.matches(&v("1.9.0.0")));
        assert!(!c.matches(&v("2.0.0.0")));
    }

    #[test]
    fn comparators_stand_alone() {
        assert!(
            Constraint::parse(">=0.1.14.0")
                .unwrap()
                .matches(&v("0.1.14.0"))
        );
        assert!(
            !Constraint::parse(">0.1.14.0")
                .unwrap()
                .matches(&v("0.1.14.0"))
        );
        assert!(
            Constraint::parse("<0.2.0.0")
                .unwrap()
                .matches(&v("0.1.99.0"))
        );
        assert!(
            Constraint::parse("<=0.1.14.0")
                .unwrap()
                .matches(&v("0.1.14.0"))
        );
    }

    #[test]
    fn a_list_is_a_conjunction() {
        let c = Constraint::parse(">=0.1.0.0, <0.2.0.0").unwrap();
        assert!(c.matches(&v("0.1.14.0")));
        assert!(!c.matches(&v("0.2.0.0")));
        assert!(!c.matches(&v("0.0.9.0")));
    }

    #[test]
    fn a_version_that_is_not_a_version_is_refused() {
        assert!(Constraint::parse("not-a-version").is_err());
    }
}
