SAVEPOINT repair_search_servers;
-- Repair only the identifiable legacy Kagi/search mismatch, never arbitrary Artifact servers.
UPDATE mcp_server SET
    command = 'aipp:search',
    environment_variables = CASE
        WHEN instr(upper(COALESCE(environment_variables, '')), 'SEARCH_ENGINE=') = 0
        THEN CASE WHEN COALESCE(environment_variables, '') = '' THEN 'SEARCH_ENGINE=kagi'
             ELSE environment_variables || char(10) || 'SEARCH_ENGINE=kagi' END
        ELSE environment_variables END
WHERE is_builtin = 1 AND command = 'aipp:artifact' AND lower(name) LIKE '%kagi%'
    AND EXISTS (SELECT 1 FROM mcp_server_tool t WHERE t.server_id = mcp_server.id AND t.tool_name = 'search_web')
    AND EXISTS (SELECT 1 FROM mcp_server_tool t WHERE t.server_id = mcp_server.id AND t.tool_name = 'fetch_url');

DELETE FROM mcp_server_tool WHERE server_id IN
    (SELECT id FROM mcp_server WHERE command = 'aipp:search' AND is_builtin = 1)
    AND tool_name IN ('get_artifact_workspace', 'show_artifact', 'capture_artifact_screenshot');

-- Upgrade only the old built-in default; retain explicitly customized budgets.
UPDATE mcp_server SET timeout = 60000
WHERE is_builtin = 1 AND command = 'aipp:search' AND (timeout IS NULL OR timeout = 20000);
RELEASE repair_search_servers;
