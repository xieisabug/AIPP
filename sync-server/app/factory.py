from __future__ import annotations

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from app.config import Settings, get_settings
from app.db import configure_database, init_db
from app.routes import admin, pull, push, status


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    settings.validate_bootstrap_security()
    configure_database(settings)
    init_db(settings)

    app = FastAPI(title="AIPP Sync Server", version="0.1.0")
    app.dependency_overrides[get_settings] = lambda: settings

    max_body = settings.max_request_body_bytes

    @app.middleware("http")
    async def limit_request_body(request: Request, call_next):
        # 基于 Content-Length 的提前拒绝；chunked 传输无此头时放行，
        # 由事件级校验（max_events_per_push / payload 校验）兜底。
        if max_body > 0:
            content_length = request.headers.get("content-length")
            if content_length is not None and content_length.isdigit() and int(content_length) > max_body:
                return JSONResponse(status_code=413, content={"detail": "request body too large"})
        return await call_next(request)

    @app.get("/health")
    def health() -> dict[str, str]:
        return {"status": "ok"}

    app.include_router(status.router)
    app.include_router(push.router)
    app.include_router(pull.router)
    app.include_router(admin.router)
    return app
