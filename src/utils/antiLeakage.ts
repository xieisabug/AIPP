/**
 * 防泄露模式脱敏工具函数
 */

/**
 * 标题脱敏：只保留第一个字，其余用 * 替换
 * @param title 原始标题
 * @returns 脱敏后的标题
 */
export function maskTitle(title: string): string {
    if (!title || title.length === 0) return "";
    return title[0] + "*".repeat(Math.max(0, title.length - 1));
}

/**
 * 内容脱敏：全部替换为星号
 * @param content 原始内容
 * @returns 脱敏后的内容
 */
export function maskContent(content: string): string {
    // 空内容也返回至少10个星号
    return "*".repeat(Math.max(10, content?.length || 0));
}

/**
 * 工具调用脱敏 - 整体替换为星号
 * @param _serverName 服务器名称
 * @param _toolName 工具名称
 * @param _parameters 参数 JSON 字符串
 * @param result 执行结果
 * @returns 脱敏后的工具调用信息
 */
export function maskToolCall(
    _serverName: string,
    _toolName: string,
    _parameters: string,
    result?: string | null
): {
    serverName: string;
    toolName: string;
    parameters: string;
    result?: string;
} {
    const maskedStr = "******"; // 固定长度的星号字符串
    return {
        serverName: maskedStr,
        toolName: maskedStr,
        parameters: maskedStr,
        result: result ? maskedStr : undefined,
    };
}
