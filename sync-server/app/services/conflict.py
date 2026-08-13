from __future__ import annotations

from app.models import SyncObject
from app.schemas import PushEvent


def is_stale_event(event: PushEvent, current: SyncObject | None) -> bool:
    if current is None:
        return False
    return (event.base_version or 0) < current.version


def should_accept_stale_event(event: PushEvent, current: SyncObject | None) -> bool:
    if not is_stale_event(event, current):
        return True
    # Message and artifact content conflicts must not be silently overwritten in MVP.
    if event.object_type in {"conversation.message", "artifacts.collection"}:
        return False
    # Other LWW-like objects can be accepted by server receive order.
    return True

