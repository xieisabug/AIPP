use super::registry::ActionRegistry;
use super::types::*;

/// Handle the `superadmin_catalog` tool call.
pub fn handle_catalog(registry: &ActionRegistry, request: &CatalogRequest) -> CatalogResponse {
    let matches = registry.search(
        request.query.as_deref(),
        request.domain.as_deref(),
        request.tag.as_deref(),
        request.risk_level,
    );

    let total = matches.len();
    let cursor = request.cursor.unwrap_or(0);
    let limit = request.limit.unwrap_or(20).min(100);

    let page: Vec<CatalogItem> = matches
        .into_iter()
        .skip(cursor)
        .take(limit)
        .map(|meta| CatalogItem {
            action_id: meta.action_id.clone(),
            domain: meta.domain.clone(),
            summary: meta.summary.clone(),
            risk_level: meta.risk_level,
            requires_approval: meta.requires_approval,
            tags: meta.tags.clone(),
        })
        .collect();

    let next_cursor = if cursor + limit < total {
        Some(cursor + limit)
    } else {
        None
    };

    CatalogResponse {
        items: page,
        total,
        next_cursor,
    }
}
