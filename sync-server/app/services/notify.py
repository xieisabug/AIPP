"""In-process account-scoped change notify hub for SSE / long-poll subscribers."""

from __future__ import annotations

import asyncio
import logging
from collections import defaultdict
from dataclasses import dataclass

logger = logging.getLogger(__name__)


@dataclass(frozen=True, slots=True)
class NotifyPayload:
    cursor: int
    device_id: str | None = None


class NotifyHub:
    """Fan-out latest sync cursor to per-account asyncio queues.

    Push handlers may run in a threadpool; use ``publish_threadsafe`` from sync code.
    """

    def __init__(self) -> None:
        self._subs: dict[str, set[asyncio.Queue[NotifyPayload]]] = defaultdict(set)
        self._lock = asyncio.Lock()
        self._loop: asyncio.AbstractEventLoop | None = None

    def bind_loop(self, loop: asyncio.AbstractEventLoop) -> None:
        self._loop = loop

    def _ensure_loop(self) -> asyncio.AbstractEventLoop | None:
        if self._loop is not None and self._loop.is_running():
            return self._loop
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            return self._loop if self._loop is not None and self._loop.is_running() else None
        self._loop = loop
        return loop

    async def subscribe(self, account_id: str) -> asyncio.Queue[NotifyPayload]:
        self._ensure_loop()
        queue: asyncio.Queue[NotifyPayload] = asyncio.Queue(maxsize=16)
        async with self._lock:
            self._subs[account_id].add(queue)
        return queue

    async def unsubscribe(self, account_id: str, queue: asyncio.Queue[NotifyPayload]) -> None:
        async with self._lock:
            buckets = self._subs.get(account_id)
            if not buckets:
                return
            buckets.discard(queue)
            if not buckets:
                self._subs.pop(account_id, None)

    async def publish(self, account_id: str, cursor: int, device_id: str | None = None) -> None:
        payload = NotifyPayload(cursor=cursor, device_id=device_id)
        async with self._lock:
            queues = list(self._subs.get(account_id, ()))
        for queue in queues:
            try:
                queue.put_nowait(payload)
            except asyncio.QueueFull:
                # Drop oldest then retry once so a slow client still sees a newer cursor.
                try:
                    queue.get_nowait()
                except asyncio.QueueEmpty:
                    pass
                try:
                    queue.put_nowait(payload)
                except asyncio.QueueFull:
                    logger.debug("notify queue still full for account %s", account_id)

    def publish_threadsafe(self, account_id: str, cursor: int, device_id: str | None = None) -> None:
        loop = self._ensure_loop()
        if loop is None or not loop.is_running():
            logger.debug("notify hub has no running loop; skip publish")
            return
        try:
            asyncio.get_running_loop()
        except RuntimeError:
            # Called from a worker thread (sync push handler).
            asyncio.run_coroutine_threadsafe(self.publish(account_id, cursor, device_id), loop)
            return
        # Already on the event loop thread — schedule without blocking.
        loop.create_task(self.publish(account_id, cursor, device_id))
