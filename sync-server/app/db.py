from __future__ import annotations

from collections.abc import Generator
from datetime import UTC, datetime, timedelta
from hashlib import sha256
from uuid import uuid4

from sqlalchemy import create_engine, event, select
from sqlalchemy.engine import Engine
from sqlalchemy.orm import Session, sessionmaker

from app.config import Settings, get_settings
from app.models import Base, SyncAccount, SyncToken

engine: Engine | None = None
SessionLocal: sessionmaker[Session] | None = None


def _set_sqlite_pragmas(dbapi_connection, _connection_record) -> None:
    cursor = dbapi_connection.cursor()
    cursor.execute("PRAGMA busy_timeout=5000")
    cursor.execute("PRAGMA journal_mode=WAL")
    cursor.execute("PRAGMA foreign_keys=ON")
    cursor.close()


def configure_database(settings: Settings | None = None) -> None:
    global engine, SessionLocal
    settings = settings or get_settings()
    settings.ensure_sqlite_parent()
    connect_args = {"check_same_thread": False} if settings.database_url.startswith("sqlite") else {}
    engine = create_engine(settings.database_url, connect_args=connect_args, future=True)
    if settings.database_url.startswith("sqlite"):
        event.listen(engine, "connect", _set_sqlite_pragmas)
    SessionLocal = sessionmaker(bind=engine, autoflush=False, autocommit=False, future=True)


def get_engine() -> Engine:
    if engine is None:
        configure_database()
    assert engine is not None
    return engine


def get_sessionmaker() -> sessionmaker[Session]:
    if SessionLocal is None:
        configure_database()
    assert SessionLocal is not None
    return SessionLocal


def init_db(settings: Settings | None = None) -> None:
    settings = settings or get_settings()
    Base.metadata.create_all(bind=get_engine())
    if settings.bootstrap_token:
        ensure_bootstrap_token(settings)


def get_db() -> Generator[Session, None, None]:
    db = get_sessionmaker()()
    try:
        yield db
    finally:
        db.close()


def token_hash(token: str) -> str:
    return sha256(token.encode("utf-8")).hexdigest()


def token_expires_at(settings: Settings) -> datetime | None:
    if settings.token_ttl_days <= 0:
        return None
    return datetime.now(UTC) + timedelta(days=settings.token_ttl_days)


def ensure_bootstrap_token(settings: Settings) -> None:
    now = datetime.now(UTC)
    with get_sessionmaker()() as db:
        account = db.get(SyncAccount, settings.bootstrap_account_id)
        if account is None:
            db.add(SyncAccount(id=settings.bootstrap_account_id, created_at=now))

        digest = token_hash(settings.bootstrap_token or "")
        existing = db.scalar(select(SyncToken).where(SyncToken.token_hash == digest))
        if existing is None:
            db.add(
                SyncToken(
                    id=str(uuid4()),
                    account_id=settings.bootstrap_account_id,
                    name="bootstrap",
                    token_hash=digest,
                    created_at=now,
                    revoked_at=None,
                    expires_at=token_expires_at(settings),
                )
            )
        db.commit()

