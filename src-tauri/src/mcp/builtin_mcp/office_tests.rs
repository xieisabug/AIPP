use super::*;

#[test]
fn test_normalize_steps_keeps_known_fields_per_format() {
    let steps = normalize_steps(
        OfficeFormat::Docx,
        &[json!({"type": "replace_text", "find": "旧", "replace": "新", "paragraph": "旧的段落", "junk": 1})],
    )
    .unwrap();
    assert_eq!(steps[0], json!({"type": "replace_text", "find": "旧", "replace": "新", "paragraph": "旧的段落"}));

    let steps = normalize_steps(
        OfficeFormat::Xlsx,
        &[json!({"type": "set_cells", "sheet": "汇总", "range": "B2:C2", "values": [[1.5, true]]})],
    )
    .unwrap();
    assert_eq!(steps[0]["values"], json!([["1.5", "TRUE"]]));

    let steps = normalize_steps(OfficeFormat::Pptx, &[json!({"type": "replace_text", "slide": 2, "find": "a", "replace": ""})]).unwrap();
    assert_eq!(steps[0]["slide"], 2);

    let step = json!({"type": "replace_text", "find": "旧", "replace": "新", "paragraph": "旧段落", "paragraphOccurrence": 2});
    assert_eq!(normalize_steps(OfficeFormat::Docx, &[step.clone()]).unwrap()[0], step);
    assert!(normalize_steps(OfficeFormat::Pptx, &[step]).is_err());
}

#[test]
fn test_normalize_steps_rejects_unsupported_and_malformed_steps() {
    let (code, message) = normalize_steps(OfficeFormat::Xlsx, &[json!({"type": "replace_text", "find": "a", "replace": "b"})]).unwrap_err();
    assert_eq!(code, "unsupported_edit");
    assert!(message.contains("set_cells"), "{message}");

    let (code, _) = normalize_steps(OfficeFormat::Docx, &[json!({"type": "replace_text", "find": "", "replace": "b"})]).unwrap_err();
    assert_eq!(code, "invalid_edit");
    let (code, _) = normalize_steps(OfficeFormat::Pptx, &[json!({"type": "replace_text", "slide": 0, "find": "a", "replace": "b"})]).unwrap_err();
    assert_eq!(code, "invalid_edit");
    let (code, _) = normalize_steps(OfficeFormat::Xlsx, &[json!({"type": "set_cells", "sheet": "s", "range": "A1", "values": [[{"x": 1}]]})]).unwrap_err();
    assert_eq!(code, "invalid_edit");
    let (code, _) = normalize_steps(OfficeFormat::Docx, &[]).unwrap_err();
    assert_eq!(code, "invalid_edit");
    let too_many: Vec<Value> = (0..=MAX_STEPS).map(|_| json!({"type": "replace_text", "find": "a", "replace": "b"})).collect();
    assert_eq!(normalize_steps(OfficeFormat::Docx, &too_many).unwrap_err().0, "invalid_edit");
}

fn pending(state: &OfficeEditState, id: &str) -> (oneshot::Receiver<()>, oneshot::Receiver<Result<Vec<u8>, (String, String)>>) {
    let (claim_tx, claim_rx) = oneshot::channel();
    let (result_tx, result_rx) = oneshot::channel();
    state.lock().insert(
        id.into(),
        PendingJob {
            format: OfficeFormat::Docx,
            file_name: "a.docx".into(),
            steps: vec![json!({"type": "replace_text"})],
            source: Arc::new(vec![1, 2, 3]),
            sha256: "s".into(),
            claimed: false,
            claim_tx: Some(claim_tx),
            result_tx: Some(result_tx),
        },
    );
    (claim_rx, result_rx)
}

#[test]
fn test_office_edit_job_is_claimed_once_and_finished_once() {
    let state = OfficeEditState::new();
    let (mut claim_rx, mut result_rx) = pending(&state, "job");
    // 未认领前不能读源、不能交结果
    assert!(state.source("job").is_none());
    assert!(!state.finish("job", Ok(vec![9])));

    let claimed = state.claim("job").unwrap();
    assert_eq!(claimed.format, "docx");
    assert!(claim_rx.try_recv().is_ok());
    assert!(state.claim("job").is_none(), "second window must not claim the same job");
    assert_eq!(*state.source("job").unwrap().0, vec![1, 2, 3]);

    assert!(state.finish("job", Ok(vec![7])));
    assert!(!state.finish("job", Err(("x".into(), "late".into()))), "a job reports only once");
    assert_eq!(result_rx.try_recv().unwrap().unwrap(), vec![7]);

    state.remove("job");
    assert!(state.claim("job").is_none());
}

#[test]
fn test_office_edit_job_guard_removes_job_when_tool_call_is_dropped() {
    let state = OfficeEditState::new();
    let _rx = pending(&state, "job");
    {
        let _guard = JobGuard { state: state.clone(), job_id: "job".into() };
    }
    assert!(state.claim("job").is_none());
}

#[test]
fn test_office_tool_definition_lists_supported_steps() {
    let defs = definitions();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, EDIT_TOOL);
    for step in ["replace_text", "replace_paragraph", "insert_paragraphs_after", "set_cells"] {
        assert!(defs[0].description.contains(step), "{step}");
    }
}
