const seenConnectionEventIds = new Set<string>();

export function claimAgentConnectionEvent(connectionEventId: string): boolean {
    if (seenConnectionEventIds.has(connectionEventId)) {
        return false;
    }
    seenConnectionEventIds.add(connectionEventId);
    return true;
}

export function resetAgentConnectionEventsForTest(): void {
    seenConnectionEventIds.clear();
}
