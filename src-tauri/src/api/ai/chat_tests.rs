use super::{prepare_empty_model_reply, EMPTY_MODEL_REPLY_NOTICE, REASONING_ONLY_MODEL_REPLY_NOTICE};

#[test]
fn test_empty_reply_classifies_bodyless_success_and_preserves_metadata() {
    for (reasoning, expected, kind) in [
        ("", EMPTY_MODEL_REPLY_NOTICE, "empty_model_reply"),
        ("  \n", EMPTY_MODEL_REPLY_NOTICE, "empty_model_reply"),
        ("先想一想", REASONING_ONLY_MODEL_REPLY_NOTICE, "reasoning_only_model_reply"),
    ] {
        let mut content = " \n".to_string();
        let metadata = prepare_empty_model_reply(
            &mut content, reasoning, 0, 0, Some(r#"{"response_id":"r1","stream_retry":{"attempt":1}}"#.into()),
        ).unwrap();
        assert_eq!(content, expected);
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(metadata["local_reply_notice"]["source"], "aipp");
        assert_eq!(metadata["local_reply_notice"]["kind"], kind);
        assert_eq!(metadata["response_id"], "r1");
        assert_eq!(metadata["stream_retry"]["attempt"], 1);
    }
}

#[test]
fn test_empty_reply_preserves_real_text_tools_and_binary_output() {
    for (body, tools, binaries) in [
        ("answer", 0, 0),
        (EMPTY_MODEL_REPLY_NOTICE, 0, 0),
        (REASONING_ONLY_MODEL_REPLY_NOTICE, 0, 0),
        ("", 1, 0),
        ("", 0, 1),
    ] {
        let mut content = body.to_string();
        let metadata = prepare_empty_model_reply(&mut content, "thinking", tools, binaries, None);
        assert_eq!(content, body);
        assert!(metadata.is_none());
    }
}
