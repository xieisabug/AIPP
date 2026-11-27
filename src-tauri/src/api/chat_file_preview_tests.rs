use super::*;
use std::io::Write;

#[test]
fn test_linked_table_requires_range_and_detects_changes() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(b"abc,def\r\n").unwrap();
    let handle = File::open(file.path()).unwrap();
    let meta = handle.metadata().unwrap();
    let entry = Arc::new(OpenFile {
        owner: "chat_ui".into(),
        file: Mutex::new(handle),
        size: meta.len(),
        modified: meta.modified().ok(),
        mime: "text/csv".into(),
        table: true,
    });
    let fetch = |range: Option<&str>| {
        let mut request = Request::builder().header(header::ORIGIN, "http://tauri.localhost");
        if let Some(value) = range {
            request = request.header(header::RANGE, value);
        }
        serve_file(Some(entry.clone()), request.body(vec![]).unwrap())
    };
    let partial = fetch(Some("bytes=4-6"));
    assert_eq!(partial.status(), 206);
    assert_eq!(partial.body(), b"def");
    assert_eq!(partial.headers()[header::CONTENT_RANGE], "bytes 4-6/9");
    assert_eq!(partial.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "http://tauri.localhost");
    assert_eq!(fetch(None).status(), 400);
    assert_eq!(fetch(Some("bytes=10-20")).status(), 416);
    file.write_all(b"new").unwrap();
    assert_eq!(fetch(Some("bytes=0-2")).status(), 409);
    assert_eq!(serve_file(None, Request::new(vec![])).status(), 404);
}
