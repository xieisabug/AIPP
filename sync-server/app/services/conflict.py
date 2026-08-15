from __future__ import annotations

from app.models import SyncObject
from app.schemas import PushEvent


def is_stale_event(event: PushEvent, current: SyncObject | None) -> bool:
    if current is None:
        return False
    return (event.base_version or 0) < current.version


def should_accept_stale_event(
    event: PushEvent, current: SyncObject | None, lww_types: set[str] | None = None
) -> bool:
    if not is_stale_event(event, current):
        return True
    # Stale writes conflict by default: silently overwriting another device's
    # changes loses user data. Only object types explicitly configured as
    # last-write-wins mergeable may be accepted in server receive order.
    return event.object_type in (lww_types or set())

