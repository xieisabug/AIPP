use super::*;

#[test]
fn test_codex_command_and_file_approvals_preserve_supported_native_variants() {
    for method in ["item/commandExecution/requestApproval", "item/fileChange/requestApproval"] {
        for decision in ["accept", "acceptForSession", "decline", "cancel"] {
            assert_eq!(
                codex_approval_response(method, &json!({}), AcpPermissionDecision::Selected(decision.into())),
                json!({"decision": decision}),
                "{method}: {decision}",
            );
        }
        assert_eq!(
            codex_approval_response(method, &json!({}), AcpPermissionDecision::Cancelled),
            json!({"decision": "cancel"}),
        );
    }
}

#[test]
fn test_codex_command_and_file_approvals_cancel_unknown_and_unsupported_extensions() {
    for method in ["item/commandExecution/requestApproval", "item/fileChange/requestApproval"] {
        for decision in [
            "", "approved", "acceptAlways", "Accept", "accept ", "futureApproval",
            "acceptWithExecpolicyAmendment", "applyNetworkPolicyAmendment",
            r#"{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["git"]}}"#,
            r#"{"applyNetworkPolicyAmendment":{"network_policy_amendment":{"host":"example.com","action":"allow"}}}"#,
        ] {
            // Even an advertised unknown string cannot bypass native validation.
            let params = json!({"availableDecisions": [decision]});
            assert_eq!(
                codex_approval_response(method, &params, AcpPermissionDecision::Selected(decision.into())),
                json!({"decision": "cancel"}),
                "{method}: {decision}",
            );
        }
    }
}

#[test]
fn test_codex_command_structured_extensions_do_not_replace_supported_string_options() {
    let params = json!({"availableDecisions": [
        "accept", "acceptForSession",
        {"acceptWithExecpolicyAmendment": {"execpolicy_amendment": ["git"]}},
        {"applyNetworkPolicyAmendment": {"network_policy_amendment": {"host": "example.com", "action": "allow"}}},
        "decline", "cancel"
    ]});
    let options = codex_permission_options("item/commandExecution/requestApproval", &params);
    assert_eq!(options.iter().map(|option| option.option_id.as_str()).collect::<Vec<_>>(),
        vec!["accept", "acceptForSession", "decline", "cancel"]);
}

#[test]
fn test_codex_permissions_approval_preserves_scope_and_denies_unknown_decisions() {
    let method = "item/permissions/requestApproval";
    let permissions = json!({"network": {"enabled": true}});
    let params = json!({"permissions": permissions});
    for (decision, scope) in [("accept", "turn"), ("acceptForSession", "session")] {
        assert_eq!(
            codex_approval_response(method, &params, AcpPermissionDecision::Selected(decision.into())),
            json!({"permissions": permissions, "scope": scope}),
        );
    }
    for decision in ["decline", "cancel", "acceptAlways", "futureApproval", "acceptWithExecpolicyAmendment"] {
        assert_eq!(
            codex_approval_response(method, &params, AcpPermissionDecision::Selected(decision.into())),
            json!({"permissions": {}}),
        );
    }
    assert_eq!(codex_approval_response(method, &params, AcpPermissionDecision::Cancelled), json!({"permissions": {}}));
}

#[test]
fn test_codex_unknown_approval_method_cannot_grant_permission() {
    assert_eq!(
        codex_approval_response("item/unknown/requestApproval", &json!({}), AcpPermissionDecision::Selected("accept".into())),
        json!({"decision": "cancel"}),
    );
}
