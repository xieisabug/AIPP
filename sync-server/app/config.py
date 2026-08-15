from __future__ import annotations

from functools import lru_cache
from pathlib import Path

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="AIPP_SYNC_", env_file=".env", extra="ignore")

    database_url: str = "sqlite:///./data/aipp-sync.db"
    base_url: str = "http://localhost:8080"
    bootstrap_account_id: str = "default"
    bootstrap_token: str | None = None
    min_client_schema_version: int = 1
    max_client_schema_version: int = 2
    max_events_per_push: int = Field(default=500, ge=1, le=5000)
    # 0 means unlimited. Self-hosted sync must not reject large conversations by default.
    max_payload_bytes: int = Field(default=0, ge=0)
    # Object types whose stale writes are accepted in server receive order (last-write-wins).
    # Empty by default: every other stale write is reported as a conflict.
    stale_lww_types: list[str] = Field(default_factory=list)

    def is_local_base_url(self) -> bool:
        host = self.base_url.split("://", 1)[-1].split("/", 1)[0].split(":", 1)[0].lower()
        return host in {"localhost", "127.0.0.1", "::1", "[::1]"}

    def validate_bootstrap_security(self) -> None:
        """Refuse to start with a missing/known-default token on a non-local deployment."""
        token = (self.bootstrap_token or "").strip()
        if self.is_local_base_url():
            if not token:
                print(
                    "[AIPP Sync] WARNING: AIPP_SYNC_BOOTSTRAP_TOKEN is not set; "
                    "no bootstrap token will be created. Set it before exposing this service."
                )
            return
        if not token or token == "dev-token":
            raise RuntimeError(
                "AIPP_SYNC_BOOTSTRAP_TOKEN must be set to a private token when base_url is not localhost; "
                "the public default 'dev-token' is not allowed."
            )

    def ensure_sqlite_parent(self) -> None:
        if not self.database_url.startswith("sqlite:///"):
            return
        raw_path = self.database_url.removeprefix("sqlite:///")
        if raw_path == ":memory:":
            return
        Path(raw_path).parent.mkdir(parents=True, exist_ok=True)


@lru_cache
def get_settings() -> Settings:
    return Settings()
