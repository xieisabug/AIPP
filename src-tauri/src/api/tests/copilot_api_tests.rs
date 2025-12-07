use crate::api::copilot_api::*;

#[cfg(test)]
mod copilot_api_tests {
    use super::*;

    /// 测试 CopilotAuthResult 的序列化
    #[test]
    fn test_copilot_auth_result_serialization() {
        let result = CopilotAuthResult {
            llm_provider_id: 1,
            access_token: "gho_test_token".to_string(),
            token_type: "bearer".to_string(),
            expires_at: None,
            scope: Some("user:email".to_string()),
        };

        let json = serde_json::to_string(&result).expect("序列化应该成功");
        assert!(json.contains("gho_test_token"));
        assert!(json.contains("bearer"));
        assert!(json.contains("user:email"));

        let deserialized: CopilotAuthResult =
            serde_json::from_str(&json).expect("反序列化应该成功");
        assert_eq!(deserialized.llm_provider_id, 1);
        assert_eq!(deserialized.access_token, "gho_test_token");
    }

    /// 测试 CopilotDeviceFlowStartResponse 的序列化
    #[test]
    fn test_device_flow_response_serialization() {
        let response = CopilotDeviceFlowStartResponse {
            device_code: "test_device_code".to_string(),
            user_code: "ABCD-1234".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            expires_in: 900,
            interval: 5,
        };

        let json = serde_json::to_string(&response).expect("序列化应该成功");
        assert!(json.contains("test_device_code"));
        assert!(json.contains("ABCD-1234"));
        assert!(json.contains("https://github.com/login/device"));

        let deserialized: CopilotDeviceFlowStartResponse =
            serde_json::from_str(&json).expect("反序列化应该成功");
        assert_eq!(deserialized.device_code, "test_device_code");
        assert_eq!(deserialized.user_code, "ABCD-1234");
        assert_eq!(deserialized.expires_in, 900);
        assert_eq!(deserialized.interval, 5);
    }
}
