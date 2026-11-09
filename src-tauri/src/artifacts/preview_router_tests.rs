use super::resolve_preview_language;

#[test]
fn test_preview_language_xml_mxfile_uses_drawio() {
    for input in [
        "<mxfile><diagram /></mxfile>",
        "<mxfile host=\"app.diagrams.net\"><diagram /></mxfile>",
        "<mxfile/>",
        "\u{feff} \r\n<mxfile />",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mxfile />",
        "<!-- Draw.io diagram -->\n<mxfile />",
        "\u{feff}<?xml version=\"1.0\"?>\n<!-- exported -->\n<!-- diagram -->\n<mxfile />",
    ] {
        assert_eq!(resolve_preview_language("xml", input), "drawio", "{input}");
    }
}

#[test]
fn test_preview_language_xml_other_roots_keep_xml() {
    for input in [
        "",
        "<root />",
        "<mxfileBackup />",
        "<root><mxfile /></root>",
        "<!-- <mxfile /> --><root />",
        "<!-- first --><root><!-- second --><mxfile /></root>",
        "<!-- unfinished <mxfile />",
        "<?xml version=\"1.0\" <mxfile />",
        "<mxfile/other />",
        "<?xml version=\"1.0\"?><root><mxfile /></root>",
        "Text before <mxfile />",
        "&lt;mxfile&gt;",
        "<MXFILE />",
    ] {
        assert_eq!(resolve_preview_language("xml", input), "xml", "{input}");
    }
}

#[test]
fn test_preview_language_explicit_types_are_preserved() {
    for lang in ["drawio", "drawio:xml", "html", "svg", "markdown", "react"] {
        assert_eq!(resolve_preview_language(lang, "<mxfile />"), lang);
    }
}
