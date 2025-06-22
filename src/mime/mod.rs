mod mapping;
mod reversed;

use std::collections::BTreeMap;
use std::sync::LazyLock;

static TABLE: LazyLock<BTreeMap<&'static str, &'static str>> =
    LazyLock::new(mapping::build_mime_types);

static REVERSE_TABLE: LazyLock<BTreeMap<&'static str, &'static str>> =
    LazyLock::new(build_reverse_mime_types);

fn build_reverse_mime_types() -> BTreeMap<&'static str, &'static str> {
    let mut x: BTreeMap<&'static str, &'static str> = TABLE.iter().map(|(k, v)| (*v, *k)).collect();
    x.extend(reversed::reverse_mime_types());
    x
}

pub struct Mime;

impl Mime {
    pub fn get(ext: &str) -> Option<&str> {
        TABLE.get(ext).copied()
    }

    pub fn get_or_fallback(ext: &str) -> &str {
        TABLE.get(ext).copied().unwrap_or(ext)
    }

    /// look up suffix by mime type
    pub fn get_suffix(mime: &str) -> Option<&str> {
        REVERSE_TABLE.get(mime).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // // print all duplicated that has more than one extension
    // #[test]
    // fn test_duplicated_extensions() {
    //     let mut x: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
    //
    //     for (ext, mime) in TABLE.iter() {
    //         x.entry(mime).or_insert(vec![]).push(ext);
    //     }
    //     for (mime, extensions) in x.iter() {
    //         if extensions.len() > 1 {
    //             println!("{}: {:?}", mime, extensions);
    //         }
    //     }
    // }

    #[test]
    fn test_get_known_extensions() {
        assert_eq!(Mime::get("html"), Some("text/html"));
        assert_eq!(Mime::get("css"), Some("text/css"));
        assert_eq!(Mime::get("js"), Some("application/javascript"));
        assert_eq!(Mime::get("json"), Some("application/json"));
        assert_eq!(Mime::get("pdf"), Some("application/pdf"));
        assert_eq!(Mime::get("png"), Some("image/png"));
        assert_eq!(Mime::get("jpg"), Some("image/jpeg"));
        assert_eq!(Mime::get("mp4"), Some("video/mp4"));
        assert_eq!(Mime::get("mp3"), Some("audio/mpeg"));
    }

    #[test]
    fn test_get_unknown_extension() {
        assert_eq!(Mime::get("unknown"), None);
        assert_eq!(Mime::get("xyz123"), None);
        assert_eq!(Mime::get(""), Some("application/octet-stream"));
    }

    #[test]
    fn test_get_or_fallback_known_extensions() {
        assert_eq!(Mime::get_or_fallback("html"), "text/html");
        assert_eq!(Mime::get_or_fallback("css"), "text/css");
        assert_eq!(Mime::get_or_fallback("js"), "application/javascript");
    }

    #[test]
    fn test_get_or_fallback_unknown_extensions() {
        assert_eq!(Mime::get_or_fallback("unknown"), "unknown");
        assert_eq!(Mime::get_or_fallback("xyz123"), "xyz123");
        assert_eq!(Mime::get_or_fallback("custom"), "custom");
    }

    #[test]
    fn test_get_suffix_known_mime_types() {
        assert_eq!(Mime::get_suffix("text/html"), Some("html"));
        assert_eq!(Mime::get_suffix("text/css"), Some("css"));
        assert_eq!(Mime::get_suffix("application/javascript"), Some("js"));
        assert_eq!(Mime::get_suffix("application/json"), Some("json"));
        assert_eq!(Mime::get_suffix("image/png"), Some("png"));
        assert_eq!(Mime::get_suffix("image/jpeg"), Some("jpg"));
    }

    #[test]
    fn test_get_suffix_unknown_mime_types() {
        assert_eq!(Mime::get_suffix("unknown/type"), None);
        assert_eq!(Mime::get_suffix("custom/mime"), None);
        assert_eq!(Mime::get_suffix("application/nonexistent"), None);
    }

    #[test]
    fn test_case_sensitivity() {
        assert_eq!(Mime::get("HTML"), None);
        assert_eq!(Mime::get("CSS"), None);
        assert_eq!(Mime::get_or_fallback("HTML"), "HTML");
    }

    #[test]
    fn test_empty_and_edge_cases() {
        assert_eq!(Mime::get(""), Some("application/octet-stream"));
        assert_eq!(Mime::get_or_fallback(""), "application/octet-stream");
        assert_eq!(Mime::get_suffix("application/octet-stream"), Some(""));
    }

    #[test]
    fn test_reverse_lookup_consistency() {
        // Test that reverse lookup works for some common types
        let test_cases = vec![
            ("html", "text/html"),
            ("css", "text/css"),
            ("js", "application/javascript"),
            ("json", "application/json"),
            ("png", "image/png"),
        ];

        for (ext, mime) in test_cases {
            assert_eq!(Mime::get(ext), Some(mime));
            assert_eq!(Mime::get_suffix(mime), Some(ext));
        }
    }

    #[test]
    fn test_table_not_empty() {
        assert!(!TABLE.is_empty(), "MIME table should not be empty");
        assert!(
            !REVERSE_TABLE.is_empty(),
            "Reverse MIME table should not be empty"
        );
    }
}
