// Utilities for analyzing artifact code snippets

// Check if code is a full React component
pub fn is_react_component(code: &str) -> bool {
    let has_import = code.contains("import") && (code.contains("react") || code.contains("React"));
    let has_function_component = code.contains("function ") && code.contains("return");
    let has_arrow_component = code.contains("const ") && code.contains("=>") && code.contains("return");
    let has_export = code.contains("export");
    let has_jsx_return = code.contains("return (") || code.contains("return <");

    (has_import || has_export) && (has_function_component || has_arrow_component) && has_jsx_return
}

// Extract React component name
pub fn extract_component_name(code: &str) -> Option<String> {
    use regex::Regex;

    if let Ok(re) = Regex::new(r"function\s+([A-Z][a-zA-Z0-9_]*)\s*\(") {
        if let Some(caps) = re.captures(code) {
            if let Some(name) = caps.get(1) { return Some(name.as_str().to_string()); }
        }
    }
    if let Ok(re) = Regex::new(r"const\s+([A-Z][a-zA-Z0-9_]*)\s*[:=]") {
        if let Some(caps) = re.captures(code) {
            if let Some(name) = caps.get(1) { return Some(name.as_str().to_string()); }
        }
    }
    if let Ok(re) = Regex::new(r"export\s+(?:default\s+)?(?:function\s+)?([A-Z][a-zA-Z0-9_]*)") {
        if let Some(caps) = re.captures(code) {
            if let Some(name) = caps.get(1) { return Some(name.as_str().to_string()); }
        }
    }
    None
}

// Check if code is a full Vue SFC component
pub fn is_vue_component(code: &str) -> bool {
    let has_template = code.contains("<template>");
    let has_script = code.contains("<script");
    let has_setup = code.contains("setup") || code.contains("defineComponent");
    let has_export_default = code.contains("export default");
    has_template && has_script && (has_setup || has_export_default)
}

// Extract Vue component name
pub fn extract_vue_component_name(code: &str) -> Option<String> {
    use regex::Regex;
    if let Ok(re) = Regex::new(r#"name\s*:\s*['"]([A-Z][a-zA-Z0-9_]*)['"]"#) {
        if let Some(caps) = re.captures(code) { if let Some(name) = caps.get(1) { return Some(name.as_str().to_string()); } }
    }
    if let Ok(re) = Regex::new(r#"defineComponent\s*\(\s*\{\s*name\s*:\s*['"]([A-Z][a-zA-Z0-9_]*)['"]"#) {
        if let Some(caps) = re.captures(code) { if let Some(name) = caps.get(1) { return Some(name.as_str().to_string()); } }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // is_react_component Tests
    // ============================================================================

    #[test]
    fn test_is_react_component_function_component() {
        let code = r#"
            import React from 'react';
            
            function MyComponent() {
                return (
                    <div>Hello World</div>
                );
            }
            
            export default MyComponent;
        "#;
        assert!(is_react_component(code));
    }

    #[test]
    fn test_is_react_component_arrow_component() {
        let code = r#"
            import React from 'react';
            
            const MyComponent = () => {
                return <div>Hello World</div>;
            };
            
            export default MyComponent;
        "#;
        assert!(is_react_component(code));
    }

    #[test]
    fn test_is_react_component_without_import() {
        let code = r#"
            function MyComponent() {
                return (
                    <div>Hello World</div>
                );
            }
            
            export default MyComponent;
        "#;
        // Without import but has export - should still be detected
        assert!(is_react_component(code));
    }

    #[test]
    fn test_is_react_component_plain_js() {
        let code = r#"
            function add(a, b) {
                return a + b;
            }
        "#;
        assert!(!is_react_component(code));
    }

    #[test]
    fn test_is_react_component_jsx_fragment() {
        let code = r#"<div>Just JSX</div>"#;
        assert!(!is_react_component(code));
    }

    #[test]
    fn test_is_react_component_with_hooks() {
        let code = r#"
            import { useState, useEffect } from 'react';
            
            function Counter() {
                const [count, setCount] = useState(0);
                return (
                    <button onClick={() => setCount(count + 1)}>
                        Count: {count}
                    </button>
                );
            }
            
            export default Counter;
        "#;
        assert!(is_react_component(code));
    }

    // ============================================================================
    // extract_component_name Tests
    // ============================================================================

    #[test]
    fn test_extract_component_name_function() {
        let code = r#"function MyComponent() { return <div />; }"#;
        assert_eq!(extract_component_name(code), Some("MyComponent".to_string()));
    }

    #[test]
    fn test_extract_component_name_const() {
        let code = r#"const MyComponent = () => { return <div />; }"#;
        assert_eq!(extract_component_name(code), Some("MyComponent".to_string()));
    }

    #[test]
    fn test_extract_component_name_const_with_type() {
        let code = r#"const MyComponent: React.FC = () => { return <div />; }"#;
        assert_eq!(extract_component_name(code), Some("MyComponent".to_string()));
    }

    #[test]
    fn test_extract_component_name_export_default() {
        let code = r#"export default function Dashboard() { return <div />; }"#;
        assert_eq!(extract_component_name(code), Some("Dashboard".to_string()));
    }

    #[test]
    fn test_extract_component_name_export_named() {
        let code = r#"export function Header() { return <div />; }"#;
        assert_eq!(extract_component_name(code), Some("Header".to_string()));
    }

    #[test]
    fn test_extract_component_name_lowercase() {
        // Component names should start with uppercase
        let code = r#"function myhelper() { return null; }"#;
        assert_eq!(extract_component_name(code), None);
    }

    #[test]
    fn test_extract_component_name_no_match() {
        let code = r#"const value = 42;"#;
        assert_eq!(extract_component_name(code), None);
    }

    #[test]
    fn test_extract_component_name_complex() {
        let code = r#"
            import React from 'react';
            
            // Some comment
            function UserProfile({ name, email }) {
                return (
                    <div className="profile">
                        <h1>{name}</h1>
                        <p>{email}</p>
                    </div>
                );
            }
            
            export default UserProfile;
        "#;
        assert_eq!(extract_component_name(code), Some("UserProfile".to_string()));
    }

    // ============================================================================
    // is_vue_component Tests
    // ============================================================================

    #[test]
    fn test_is_vue_component_options_api() {
        let code = r#"
            <template>
                <div>{{ message }}</div>
            </template>
            
            <script>
            export default {
                data() {
                    return { message: 'Hello' };
                }
            }
            </script>
        "#;
        assert!(is_vue_component(code));
    }

    #[test]
    fn test_is_vue_component_composition_api() {
        let code = r#"
            <template>
                <div>{{ count }}</div>
            </template>
            
            <script setup>
            import { ref } from 'vue';
            const count = ref(0);
            </script>
        "#;
        assert!(is_vue_component(code));
    }

    #[test]
    fn test_is_vue_component_define_component() {
        let code = r#"
            <template>
                <div>Component</div>
            </template>
            
            <script>
            import { defineComponent } from 'vue';
            
            export default defineComponent({
                name: 'MyComponent'
            });
            </script>
        "#;
        assert!(is_vue_component(code));
    }

    #[test]
    fn test_is_vue_component_missing_template() {
        let code = r#"
            <script>
            export default {
                data() { return {}; }
            }
            </script>
        "#;
        assert!(!is_vue_component(code));
    }

    #[test]
    fn test_is_vue_component_missing_script() {
        let code = r#"
            <template>
                <div>Just template</div>
            </template>
        "#;
        assert!(!is_vue_component(code));
    }

    #[test]
    fn test_is_vue_component_plain_html() {
        let code = r#"<div>Just HTML</div>"#;
        assert!(!is_vue_component(code));
    }

    // ============================================================================
    // extract_vue_component_name Tests
    // ============================================================================

    #[test]
    fn test_extract_vue_component_name_options() {
        let code = r#"
            export default {
                name: 'MyVueComponent',
                data() { return {}; }
            }
        "#;
        assert_eq!(extract_vue_component_name(code), Some("MyVueComponent".to_string()));
    }

    #[test]
    fn test_extract_vue_component_name_double_quotes() {
        let code = r#"
            export default {
                name: "Dashboard",
                methods: {}
            }
        "#;
        assert_eq!(extract_vue_component_name(code), Some("Dashboard".to_string()));
    }

    #[test]
    fn test_extract_vue_component_name_define_component() {
        let code = r#"
            import { defineComponent } from 'vue';
            
            export default defineComponent({
                name: 'UserCard',
                props: ['user']
            });
        "#;
        assert_eq!(extract_vue_component_name(code), Some("UserCard".to_string()));
    }

    #[test]
    fn test_extract_vue_component_name_no_name() {
        let code = r#"
            export default {
                data() { return { count: 0 }; }
            }
        "#;
        assert_eq!(extract_vue_component_name(code), None);
    }

    #[test]
    fn test_extract_vue_component_name_lowercase() {
        // Vue component names should start with uppercase
        let code = r#"
            export default {
                name: 'myComponent',
                data() { return {}; }
            }
        "#;
        // Lowercase starting name won't match the regex
        assert_eq!(extract_vue_component_name(code), None);
    }

    // ============================================================================
    // Edge Cases
    // ============================================================================

    #[test]
    fn test_empty_code() {
        assert!(!is_react_component(""));
        assert!(!is_vue_component(""));
        assert_eq!(extract_component_name(""), None);
        assert_eq!(extract_vue_component_name(""), None);
    }

    #[test]
    fn test_whitespace_only() {
        let code = "   \n\t\n   ";
        assert!(!is_react_component(code));
        assert!(!is_vue_component(code));
        assert_eq!(extract_component_name(code), None);
        assert_eq!(extract_vue_component_name(code), None);
    }

    #[test]
    fn test_react_with_typescript() {
        let code = r#"
            import React, { FC } from 'react';
            
            interface Props {
                name: string;
            }
            
            const Greeting: FC<Props> = ({ name }) => {
                return <div>Hello, {name}!</div>;
            };
            
            export default Greeting;
        "#;
        assert!(is_react_component(code));
        assert_eq!(extract_component_name(code), Some("Greeting".to_string()));
    }

    #[test]
    fn test_vue_with_typescript() {
        let code = r#"
            <template>
                <div>{{ message }}</div>
            </template>
            
            <script setup lang="ts">
            import { ref } from 'vue';
            
            const message = ref<string>('Hello TypeScript');
            </script>
        "#;
        assert!(is_vue_component(code));
    }
}
