use super::{builtin_templates, get_builtin_tools_for_command};
use crate::api::ai_api::build_tools_with_mapping;
use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};
use serde_json::{json, Value};

fn check_schema(node: &Value, path: &str) {
    let schema = node.as_object().unwrap_or_else(|| panic!("{path} is not a schema"));
    for key in schema.keys() {
        assert!(matches!(key.as_str(), "type" | "description" | "properties" | "required" | "items" | "enum"),
            "{path}: check provider compatibility before sending {key}");
    }
    assert!(matches!(node["type"].as_str(), Some("object" | "array" | "string" | "number" | "integer" | "boolean")),
        "{path}: missing or unsupported type");
    if let Some(values) = schema.get("enum") {
        assert_eq!(node["type"], "string", "{path}");
        let values = values.as_array().expect("enum must be an array");
        assert!(!values.is_empty() && values.iter().all(Value::is_string), "{path}");
    }
    if node["type"] == "array" {
        check_schema(&node["items"], &format!("{path}.items"));
    }
    if let Some(properties) = schema.get("properties") {
        assert_eq!(node["type"], "object", "{path}");
        for (key, field) in properties.as_object().expect("properties must be an object") {
            check_schema(field, &format!("{path}.{key}"));
        }
    }
    if let Some(required) = schema.get("required") {
        for key in required.as_array().expect("required must be an array") {
            assert!(node["properties"].get(key.as_str().expect("required key")).is_some(),
                "{path}: unknown required property {key}");
        }
    }
}

#[test]
fn test_builtin_tool_schemas_in_model_requests() {
    let templates = builtin_templates();
    let mut count = 0;
    for template in &templates {
        let definitions = get_builtin_tools_for_command(&template.command);
        assert!(!definitions.is_empty(), "{}", template.command);
        let server = MCPServerWithTools {
            id: 1, name: template.name.clone(), summary: String::new(),
            command: Some(template.command.clone()), is_enabled: true,
            tools: definitions.iter().map(|d| MCPToolInfo {
                id: 1, name: d.name.clone(), description: d.description.clone(),
                is_enabled: true, is_auto_run: false, parameters: d.input_schema.to_string(),
            }).collect(),
        };
        let (tools, _) = build_tools_with_mapping(&[server]);
        assert_eq!(tools.len(), definitions.len());
        for tool in tools {
            let schema = tool.schema.as_ref().unwrap();
            assert_eq!(tool.strict, Some(false));
            assert_eq!(schema["type"], "object", "{}", tool.name.as_str());
            check_schema(schema, tool.name.as_str());
            count += 1;
        }
    }
    println!("Checked {count} builtin tools across {} servers", templates.len());
}

#[test]
fn test_builtin_tool_schemas_preserve_parameter_names_and_map_guidance() {
    let mut schema = json!({"type":"object", "properties":{
        "default":{"type":"integer","minimum":0},
        "headers":{"type":"object","additionalProperties":{"type":"string"}}
    }});
    super::prepare_model_schema(&mut schema);
    assert_eq!(schema["properties"]["default"]["type"], "integer");
    assert!(schema["properties"]["headers"]["description"].as_str().unwrap().contains("string"));
}
