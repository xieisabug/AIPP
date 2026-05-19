from __future__ import annotations

from datetime import datetime
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field

Operation = Literal["upsert", "delete"]


class SyncPayload(BaseModel):
    model_config = ConfigDict(extra="allow")

    fields: dict[str, Any] = Field(default_factory=dict)
    refs: dict[str, Any] = Field(default_factory=dict)


class PushEvent(BaseModel):
    event_id: str
    object_type: str
    object_id: str
    operation: Operation
    base_version: int | None = 0
    local_version: int | None = None
    payload: SyncPayload | None = None
    created_at: datetime | None = None
    client_schema_version: int = 1
    object_schema_version: int = 1


class PushRequest(BaseModel):
    device_id: str
    device_name: str | None = None
    events: list[PushEvent] = Field(default_factory=list)


class AcceptedEvent(BaseModel):
    event_id: str
    object_type: str
    object_id: str
    server_version: int
    server_seq: int


class ConflictEvent(BaseModel):
    event_id: str
    object_type: str
    object_id: str
    server_version: int
    server_payload: dict[str, Any] | None


class RejectedEvent(BaseModel):
    event_id: str
    reason: str


class PushResponse(BaseModel):
    accepted: list[AcceptedEvent] = Field(default_factory=list)
    conflicts: list[ConflictEvent] = Field(default_factory=list)
    rejected: list[RejectedEvent] = Field(default_factory=list)


class PullChange(BaseModel):
    seq: int
    event_id: str
    device_id: str
    object_type: str
    object_id: str
    operation: Operation
    version: int
    payload: dict[str, Any] | None
    deleted_at: datetime | None
    created_at: datetime


class PullResponse(BaseModel):
    cursor: int
    has_more: bool
    changes: list[PullChange] = Field(default_factory=list)


class StatusResponse(BaseModel):
    account_id: str
    object_count: int
    latest_cursor: int
    min_client_schema_version: int
    max_client_schema_version: int
    remote_empty: bool
    device_registered: bool

