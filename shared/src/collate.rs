//! String ordering identical to JavaScript's `String.prototype.localeCompare` (ICU root / en).
//!
//! Sort order is not cosmetic here: the client hashes save files in sorted order to detect
//! changes, so a different order would look like a local change to the Node.js client.

use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::preferences::CollationNumericOrdering;
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use std::cmp::Ordering;
use std::sync::LazyLock;

static DEFAULT: LazyLock<CollatorBorrowed<'static>> =
    LazyLock::new(|| Collator::try_new(CollatorPreferences::default(), CollatorOptions::default()).expect("collation data"));

static NATURAL: LazyLock<CollatorBorrowed<'static>> = LazyLock::new(|| {
    let mut prefs = CollatorPreferences::default();
    prefs.numeric_ordering = Some(CollationNumericOrdering::True);
    let mut opts = CollatorOptions::default();
    opts.strength = Some(Strength::Primary);
    Collator::try_new(prefs, opts).expect("collation data")
});

/// `a.localeCompare(b)`
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    DEFAULT.compare(a, b)
}

/// `a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' })`, so "Disc 2" < "Disc 10".
pub fn natural_compare(a: &str, b: &str) -> Ordering {
    NATURAL.compare(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_like_javascript() {
        // Values checked against node: ['b','A','a','_x','B','10','9'].sort((x, y) => x.localeCompare(y))
        let mut v = vec!["b", "A", "a", "_x", "B", "10", "9"];
        v.sort_by(|x, y| locale_compare(x, y));
        assert_eq!(v, vec!["_x", "10", "9", "a", "A", "b", "B"]);
        let mut f = vec!["0/b.mcd", "0/A.mcd", "0/a-1.mcd", "0/a_1.mcd", "1/x", "0/Z"];
        f.sort_by(|x, y| locale_compare(x, y));
        assert_eq!(f, vec!["0/a_1.mcd", "0/a-1.mcd", "0/A.mcd", "0/b.mcd", "0/Z", "1/x"]);
        let mut d = vec!["Disc 10", "disc 2", "Disc 1"];
        d.sort_by(|x, y| natural_compare(x, y));
        assert_eq!(d, vec!["Disc 1", "disc 2", "Disc 10"]);
    }
}
