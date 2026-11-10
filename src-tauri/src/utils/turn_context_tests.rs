use super::turn_context::{append, attribute, context, elements, last_element, split, visible, visible_sql, wrap};
use rusqlite::Connection;

#[test]
fn test_turn_context_round_trip_keeps_user_text_and_attachments() {
    let block = wrap(["<current_time>2026-11-10 17:00</current_time>", "", "  <long_term_memory>- #3 v1：画图</long_term_memory>  "]).unwrap();
    let body = "画个流程图\n<fileattachment name=\"a.txt\">附件全文</fileattachment>";
    let stored = append(body, Some(block.as_str()));
    assert!(stored.starts_with(body));
    assert_eq!(visible(&stored), body);
    let inner = context(&stored).unwrap();
    assert!(inner.starts_with("<current_time>"), "说明行不算正文：{inner}");
    assert!(inner.ends_with("</long_term_memory>"));
    assert_eq!(wrap(["", "  "]), None);
    assert_eq!(append(body, None), body);
    assert_eq!(append("", Some(block.as_str())), block);
}

#[test]
fn test_turn_context_requires_trailing_block() {
    // 用户自己打出标签、或标签后面还有正文，都不算附加上下文。
    let typed = "怎么用 <aipp_turn_context> 这个标签？";
    assert_eq!(split(typed), (typed, None));
    let not_last = "<aipp_turn_context>x</aipp_turn_context>\n后面还有话";
    assert_eq!(split(not_last), (not_last, None));
    assert_eq!(visible("只有原话"), "只有原话");
}

#[test]
fn test_turn_context_sql_matches_rust() {
    let conn = Connection::open_in_memory().unwrap();
    let stored = append("问题\n", wrap(["<current_time>t</current_time>"]).as_deref());
    for text in [stored.as_str(), "没有附加上下文", "怎么用 <aipp_turn_context> 这个标签？", ""] {
        let sql: String = conn
            .query_row(&format!("SELECT {}", visible_sql("?1")), [text], |r| r.get(0))
            .unwrap();
        assert_eq!(sql, visible(text), "{text:?}");
    }
}

#[test]
fn test_turn_context_elements_and_attributes() {
    let text = "<recent_workx>不算</recent_workx>\n<recent_work>\n- a\n</recent_work>说明\n<recent_work>\n- b\n</recent_work>";
    assert_eq!(elements(text, "recent_work"), vec!["<recent_work>\n- a\n</recent_work>", "<recent_work>\n- b\n</recent_work>"]);
    let contexts = vec![
        "<available_work ref=\"task:1\">旧</available_work>".to_string(),
        "<recent_work>新</recent_work>\n<available_work ref=\"task:2\">最新</available_work>".to_string(),
        "<current_time>t</current_time>".to_string(),
    ];
    assert_eq!(last_element(&contexts, &["recent_work", "available_work"]), Some("<available_work ref=\"task:2\">最新</available_work>"));
    assert_eq!(last_element(&contexts, &["current_butler_goal"]), None);
    let memory = "<long_term_memory full=\"true\" as_of=\"1794300992374\">\n- #3 v1：x\n</long_term_memory>";
    assert_eq!(attribute(memory, "as_of"), Some("1794300992374"));
    assert_eq!(attribute(memory, "full"), Some("true"));
    assert_eq!(attribute(memory, "missing"), None);
}
