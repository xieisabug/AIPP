from __future__ import annotations

from fastapi import FastAPI

from app.config import Settings, get_settings
from app.db import configure_database, init_db
from app.routes import pull, push, status


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    configure_database(settings)
    init_db(settings)

    app = FastAPI(title="AIPP Sync Server", version="0.1.0")
    app.dependency_overrides[get_settings] = lambda: settings

    @app.get("/health")
    def health() -> dict[str, str]:
        return {"status": "ok"}

    app.include_router(status.router)
    app.include_router(push.router)
    app.include_router(pull.router)
    return app
