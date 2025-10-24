export function getErrorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object") {
    const anyErr = err as any;
    if (typeof anyErr.message === "string") return anyErr.message;
    if (typeof anyErr.error === "string") return anyErr.error;
    if (typeof anyErr.reason === "string") return anyErr.reason;
    // Handle Tauri AppError serialized as a single-key object, e.g. {"ParseError":"..."}
    const keys = Object.keys(anyErr);
    if (keys.length === 1 && typeof anyErr[keys[0]] === "string") {
      return String(anyErr[keys[0]]);
    }
    // Handle nested shapes like { Err: { Variant: "..." } }
    if (keys.length === 1 && anyErr[keys[0]] && typeof anyErr[keys[0]] === "object") {
      const inner = anyErr[keys[0]];
      const innerKeys = Object.keys(inner);
      if (innerKeys.length === 1 && typeof inner[innerKeys[0]] === "string") {
        return String(inner[innerKeys[0]]);
      }
    }
    try {
      return JSON.stringify(err);
    } catch {
      return String(err);
    }
  }
  return String(err);
}

export function isInsufficientMessagesError(err: unknown): boolean {
  const msg = getErrorMessage(err);
  return (
    msg.includes("InsufficientMessages") ||
    msg.includes("消息数量不足") ||
    msg.includes("不足以生成标题")
  );
}
