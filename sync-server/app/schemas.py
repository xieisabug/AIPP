from __future__ import annotations

from datetime import datetime
from typing import Annotated, Any, Literal

from pydantic import BaseModel, ConfigDict, Field, StringConstraints

Operation = Literal["upsert", "delete"]

# 同步 ID 的允许字符集。注意 object_id 可能是 `natural:<base64>` 形式
# （标准 base64 含 +/=），因此字符集必须包含它们。
SyncId = Annotated[str, StringConstraints(min_length=1, max_length=128, pattern=r"^[A-Za-z0-9._:+/=\-]+$")]


class SyncPayload(BaseModel):
    model_config = ConfigDict(extra="allow")

    fields: dict[str, Any] = Field(default_factory=dict)
    refs: dict[str, Any] = Field(default_factory=dict)


class PushEvent(BaseModel):
    event_id: SyncId
    object_type: SyncId
    object_id: SyncId
    operation: Operation
    base_version: int | None = Field(default=0, ge=0)
    local_version: int | None = Field(default=None, ge=0)
    payload: SyncPayload | None = None
    created_at: datetime | None = None
    client_schema_version: int = Field(default=1, ge=0)
    object_schema_version: int = Field(default=1, ge=0)


class PushRequest(BaseModel):
    device_id: SyncId
    device_name: str | None = Field(default=None, max_length=256)
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
    # "delete" means the server-side object is a tombstone (payload is None);
    # clients must not guess this from server_payload being empty.
    server_operation: Operation = "upsert"


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

