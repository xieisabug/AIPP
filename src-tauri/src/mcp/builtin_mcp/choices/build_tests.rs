use super::*;

#[test]
fn test_views_share_only_the_same_snapshot_and_release_by_owner() {
    let key = (71, "request-a".to_owned(), "one".to_owned());
    let (first, a) = reserve_view("inline-test", key.clone());
    let (second, b) = reserve_view("window-test", key);
    let (third, c) = reserve_view("inline-test", (71, "request-a".into(), "two".into()));
    assert!(Arc::ptr_eq(&a, &b));
    assert!(!Arc::ptr_eq(&a, &c));
    assert_ne!(first, second);
    close_window("inline-test");
    let views = VIEWS.lock().unwrap();
    assert!(!views.contains_key(&first));
    assert!(!views.contains_key(&third));
    assert!(views.contains_key(&second));
    drop(views);
    drop(OpeningView(Some(second.clone())));
    assert!(!VIEWS.lock().unwrap().contains_key(&second));
}

#[tokio::test]
#[ignore = "requires AIPP_CHOICE_TEST_BUN or a local Bun on PATH"]
async fn test_bun_survives_one_view_closing_and_exits_after_the_last() {
    let exe = std::env::var_os("AIPP_CHOICE_TEST_BUN").map(PathBuf::from)
        .unwrap_or_else(|| which::which("bun").expect("locate Bun"));
    let key = (12, "shared-process-test".to_owned(), "candidate".to_owned());
    let data = r#"{"index.html":"T0s="}"#;
    let (a, b) = tokio::join!(
        open_snapshot("first-window", key.clone(), data, || Ok(exe.clone())),
        open_snapshot("second-window", key, data, || Ok(exe.clone())),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.url, b.url);
    assert_ne!(a.lease, b.lease);
    close_window("first-window");
    close_choice_build(a.lease).await;
    let url = reqwest::Url::parse(&b.url).unwrap();
    let port = url.port().unwrap();
    let body = reqwest::Client::builder().no_proxy().build().unwrap()
        .get(format!("http://127.0.0.1:{port}/"))
        .header("Host", format!("{}:{port}", url.host_str().unwrap()))
        .send().await.unwrap().error_for_status().unwrap().text().await.unwrap();
    assert_eq!(body, "OK");
    close_choice_build(b.lease).await;
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err());
}
