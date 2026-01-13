//! Tests for Agent tool set

#[cfg(test)]
mod tests {
    use crate::mcp::builtin_mcp::agent::types::*;
    use crate::mcp::builtin_mcp::templates::{get_builtin_tools_for_command, BuiltinToolInfo};

    // ============================================
    // Agent Template Tests
    // ============================================

    #[test]
    fn test_agent_tools_registered() {
        let tools = get_builtin_tools_for_command("aipp:agent");
        assert!(!tools.is_empty(), "Agent command should have tools");
    }

    #[test]
    fn test_load_skill_tool_exists() {
        let tools = get_builtin_tools_for_command("aipp:agent");
        let load_skill = tools.iter().find(|t| t.name == "load_skill");
        assert!(load_skill.is_some(), "load_skill tool should exist");
    }

    #[test]
    fn test_load_skill_tool_schema() {
        let tools = get_builtin_tools_for_command("aipp:agent");
        let load_skill = tools.iter().find(|t| t.name == "load_skill").unwrap();

        assert!(!load_skill.description.is_empty());

        let schema = &load_skill.input_schema;
        assert_eq!(schema["type"], "object");

        // Check properties exist
        assert!(schema["properties"]["command"].is_object(), "command property should exist");
        assert!(
            schema["properties"]["source_type"].is_object(),
            "source_type property should exist"
        );

        // Check required fields
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|r| r == "command"), "command should be required");
        assert!(required.iter().any(|r| r == "source_type"), "source_type should be required");
    }

    // ============================================
    // LoadSkillRequest Tests
    // ============================================

    #[test]
    fn test_load_skill_request_creation() {
        let request =
            LoadSkillRequest { command: "test-skill".to_string(), source_type: "aipp".to_string() };

        assert_eq!(request.command, "test-skill");
        assert_eq!(request.source_type, "aipp");
    }

    #[test]
    fn test_load_skill_request_serialization() {
        let request = LoadSkillRequest {
            command: "pdf".to_string(),
            source_type: "claude_code_agents".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("pdf"));
        assert!(json.contains("claude_code_agents"));
    }

    #[test]
    fn test_load_skill_request_deserialization() {
        let json = r#"{"command": "xlsx", "source_type": "codex"}"#;
        let request: LoadSkillRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.command, "xlsx");
        assert_eq!(request.source_type, "codex");
    }

    // ============================================
    // LoadSkillResponse Tests
    // ============================================

    #[test]
    fn test_load_skill_response_found() {
        let response = LoadSkillResponse {
            identifier: "aipp:test-skill".to_string(),
            content: "# Test Skill\nInstructions here".to_string(),
            additional_files: vec![],
            found: true,
            error: None,
        };

        assert!(response.found);
        assert!(response.error.is_none());
        assert!(!response.content.is_empty());
    }

    #[test]
    fn test_load_skill_response_not_found() {
        let response = LoadSkillResponse {
            identifier: "aipp:missing".to_string(),
            content: String::new(),
            additional_files: vec![],
            found: false,
            error: Some("Skill not found".to_string()),
        };

        assert!(!response.found);
        assert!(response.error.is_some());
        assert!(response.content.is_empty());
    }

    #[test]
    fn test_load_skill_response_with_additional_files() {
        let response = LoadSkillResponse {
            identifier: "aipp:complex-skill".to_string(),
            content: "# Complex Skill".to_string(),
            additional_files: vec![
                SkillFileContent {
                    path: "helpers.py".to_string(),
                    content: "def helper(): pass".to_string(),
                },
                SkillFileContent {
                    path: "config.json".to_string(),
                    content: r#"{"key": "value"}"#.to_string(),
                },
            ],
            found: true,
            error: None,
        };

        assert_eq!(response.additional_files.len(), 2);
        assert_eq!(response.additional_files[0].path, "helpers.py");
        assert_eq!(response.additional_files[1].path, "config.json");
    }

    // ============================================
    // SkillFileContent Tests
    // ============================================

    #[test]
    fn test_skill_file_content_creation() {
        let file = SkillFileContent {
            path: "script.sh".to_string(),
            content: "#!/bin/bash\necho Hello".to_string(),
        };

        assert_eq!(file.path, "script.sh");
        assert!(file.content.contains("echo Hello"));
    }

    #[test]
    fn test_skill_file_content_serialization() {
        let file =
            SkillFileContent { path: "data.txt".to_string(), content: "test content".to_string() };

        let json = serde_json::to_string(&file).unwrap();
        let parsed: SkillFileContent = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.path, file.path);
        assert_eq!(parsed.content, file.content);
    }
}
