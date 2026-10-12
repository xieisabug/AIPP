"""SSE change notifications: clients subscribe and pull when cursor advances."""

from __future__ import annotations

import asyncio
import json
import logging
from collections.abc import AsyncIterator

from fastapi import APIRouter, Depends, Query, Request
from fastapi.responses import StreamingResponse
from sqlalchemy.orm import Session

from app.auth import AuthContext, ensure_device, require_auth, require_device_id
from app.db import get_db
from app.services.cursor import latest_cursor
from app.services.notify import NotifyHub, NotifyPayload

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/v1/sync", tags=["sync"])

KEEPALIVE_SECS = 20.0
# Cap a single SSE connection so proxies / load balancers do not hold forever.
MAX_STREAM_SECS = 300.0


def get_notify_hub(request: Request) -> NotifyHub:
    return request.app.state.notify_hub


@router.get("/notify")
async def notify_stream(
    request: Request,
    cursor: int = Query(default=0, ge=0),
    auth: AuthContext = Depends(require_auth),
    device_id: str = Depends(require_device_id),
    db: Session = Depends(get_db),
    hub: NotifyHub = Depends(get_notify_hub),
) -> StreamingResponse:
    ensure_device(db, auth, device_id)
    # Snapshot current server cursor before opening the queue so we do not miss
    # a concurrent push between this read and subscribe.
    server_cursor = latest_cursor(db, auth.account_id)
    db.commit()

    async def event_gen() -> AsyncIterator[str]:
        queue = await hub.subscribe(auth.account_id)
        try:
            # Catch-up: if the client is behind, nudge immediately then end
            # so the client pulls and reconnects with an updated cursor.
            if server_cursor > cursor:
                yield _sse_event(
                    NotifyPayload(cursor=server_cursor, device_id=None),
                    event="sync",
                )
                return
            deadline = asyncio.get_running_loop().time() + MAX_STREAM_SECS
            while True:
                if await request.is_disconnected():
                    break
                remaining = deadline - asyncio.get_running_loop().time()
                if remaining <= 0:
                    yield _sse_comment("recycle")
                    break
                timeout = min(KEEPALIVE_SECS, remaining)
                try:
                    payload = await asyncio.wait_for(queue.get(), timeout=timeout)
                except TimeoutError:
                    yield _sse_comment("keepalive")
                    continue
                # Skip echoes of this device's own push — local outbox already applied.
                if payload.device_id and payload.device_id == device_id:
                    continue
                if payload.cursor <= cursor:
                    continue
                yield _sse_event(payload, event="sync")
                # After one actionable event the client should pull; recycle so
                # reconnects carry an updated cursor query param.
                break
        finally:
            await hub.unsubscribe(auth.account_id, queue)

    return StreamingResponse(
        event_gen(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no",
        },
    )


def _sse_event(payload: NotifyPayload, *, event: str) -> str:
    data = json.dumps(
        {"cursor": payload.cursor, "device_id": payload.device_id},
        separators=(",", ":"),
    )
    return f"event: {event}\ndata: {data}\n\n"


def _sse_comment(text: str) -> str:
    return f": {text}\n\n"
