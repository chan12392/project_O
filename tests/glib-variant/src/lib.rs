#[cfg(test)]
mod tests {
    use glib::{variant::ToVariant, Variant};

    fn strings(values: &[&str]) -> Variant {
        values.to_variant()
    }

    #[test]
    fn forward_reverse_and_last_return_real_strings() {
        let variant = strings(&["", "plain", "한글 🐯"]);
        let expected = ["", "plain", "한글 🐯"];
        assert_eq!(
            variant.array_iter_str().unwrap().collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            variant.array_iter_str().unwrap().rev().collect::<Vec<_>>(),
            expected.into_iter().rev().collect::<Vec<_>>()
        );
        assert_eq!(variant.array_iter_str().unwrap().last(), Some("한글 🐯"));
    }

    #[test]
    fn alternating_ends_and_skips_exhaust_once() {
        let variant = strings(&["a", "b", "c", "d", "e"]);
        let mut iter = variant.array_iter_str().unwrap();
        assert_eq!(iter.len(), 5);
        assert_eq!(iter.next(), Some("a"));
        assert_eq!(iter.next_back(), Some("e"));
        assert_eq!(iter.nth(1), Some("c"));
        assert_eq!(iter.nth_back(0), Some("d"));
        assert_eq!(iter.len(), 0);
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn empty_singleton_and_out_of_range_are_safe() {
        let empty = strings(&[]);
        let mut iter = empty.array_iter_str().unwrap();
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
        let one = strings(&["only"]);
        assert_eq!(one.array_iter_str().unwrap().next_back(), Some("only"));
        assert_eq!(one.array_iter_str().unwrap().nth(1), None);
        assert_eq!(one.array_iter_str().unwrap().nth_back(1), None);
        assert!(123u32.to_variant().array_iter_str().is_err());
    }
}
