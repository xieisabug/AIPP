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
    bootstrap_token: str | None = "dev-token"
    min_client_schema_version: int = 1
    max_client_schema_version: int = 1
    max_events_per_push: int = Field(default=500, ge=1, le=5000)
    # 0 means unlimited. Self-hosted sync must not reject large conversations by default.
    max_payload_bytes: int = Field(default=0, ge=0)

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
