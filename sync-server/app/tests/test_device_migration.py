from __future__ import annotations

from datetime import datetime
from pathlib import Path

import pytest
import sqlalchemy as sa
from alembic import command
from alembic.config import Config

from app.models import Base


def migration_config(connection: sa.Connection) -> Config:
    config = Config()
    config.set_main_option("script_location", str(Path(__file__).resolve().parents[2] / "alembic"))
    config.attributes["connection"] = connection
    return config


@pytest.fixture
def legacy_database():
    # Execute the actual migration chain against SQLite, not metadata.create_all
    # or mocked DDL. The database is memory-only, including rollback tests.
    engine = sa.create_engine("sqlite:///:memory:")
    with engine.connect() as connection:
        connection.exec_driver_sql("PRAGMA foreign_keys=ON")
        config = migration_config(connection)
        command.upgrade(config, "0003_sync_token_expires_at")
        now = datetime(2026, 11, 11, 1, 2, 3).isoformat(" ")
        for account_id in ("x", "y"):
            connection.execute(sa.text(
                "INSERT INTO sync_account (id, created_at) VALUES (:id, :now)"
            ), {"id": account_id, "now": now})
        for device_id, revoked_at in (("active", None), ("revoked", now)):
            connection.execute(sa.text(
                "INSERT INTO sync_device "
                "(id, account_id, name, created_at, last_seen_at, revoked_at) "
                "VALUES (:id, 'x', :name, :now, :now, :revoked)"
            ), {"id": device_id, "name": f"Original {device_id}", "now": now, "revoked": revoked_at})
        connection.execute(sa.text(
            "INSERT INTO sync_token "
            "(id, account_id, name, token_hash, created_at, revoked_at, expires_at) "
            "VALUES ('token', 'x', 'Existing token', :hash, :now, NULL, NULL)"
        ), {"hash": "a" * 64, "now": now})
        connection.execute(sa.text(
            "INSERT INTO sync_object "
            "(account_id, object_type, object_id, version, payload_hash, deleted_at, "
            "updated_at, updated_by_device_id) "
            "VALUES ('x', 'conversation', 'deleted', 1, :hash, :now, :now, 'active')"
        ), {"hash": "b" * 64, "now": now})
        connection.execute(sa.text(
            "INSERT INTO sync_change "
            "(account_id, event_id, object_type, object_id, operation, version, "
            "deleted_at, device_id, created_at) "
            "VALUES ('x', 'delete-event', 'conversation', 'deleted', 'delete', 1, :now, 'active', :now)"
        ), {"now": now})
        connection.commit()
        yield connection, config
    engine.dispose()


def device_rows(connection):
    return connection.execute(sa.text("SELECT * FROM sync_device ORDER BY account_id, id")).all()


def test_device_migration_preserves_rows_and_supports_account_scoped_ids(legacy_database):
    connection, config = legacy_database
    original_rows = device_rows(connection)
    other_rows = {
        table: connection.exec_driver_sql(f"SELECT * FROM {table}").all()
        for table in ("sync_account", "sync_token", "sync_object", "sync_change")
    }
    command.upgrade(config, "head")

    assert device_rows(connection) == original_rows
    for table, rows in other_rows.items():
        assert connection.exec_driver_sql(f"SELECT * FROM {table}").all() == rows
    inspector = sa.inspect(connection)
    assert inspector.get_pk_constraint("sync_device")["constrained_columns"] == ["account_id", "id"]
    assert inspector.get_unique_constraints("sync_device") == []
    foreign_key = inspector.get_foreign_keys("sync_device")[0]
    assert foreign_key["constrained_columns"] == ["account_id"]
    assert foreign_key["referred_table"] == "sync_account"
    assert foreign_key["options"]["ondelete"] == "CASCADE"
    assert connection.exec_driver_sql("PRAGMA foreign_key_check").all() == []

    connection.execute(sa.text(
        "INSERT INTO sync_device SELECT id, 'y', 'Device in Y', created_at, NULL, NULL "
        "FROM sync_device WHERE account_id = 'x' AND id = 'active'"
    ))
    connection.commit()
    with pytest.raises(sa.exc.IntegrityError):
        with connection.begin_nested():
            connection.execute(sa.text(
                "INSERT INTO sync_device SELECT * FROM sync_device WHERE account_id = 'y'"
            ))
    with pytest.raises(sa.exc.IntegrityError):
        with connection.begin_nested():
            connection.execute(sa.text(
                "INSERT INTO sync_device SELECT id, 'missing', name, created_at, NULL, NULL "
                "FROM sync_device WHERE account_id = 'y'"
            ))
    command.upgrade(config, "head")
    assert len(device_rows(connection)) == 3
    # Cascades still work without deleting the other account's registration.
    connection.execute(sa.text("DELETE FROM sync_account WHERE id = 'y'"))
    assert device_rows(connection) == original_rows


def test_device_migration_downgrade_preserves_rows_when_ids_are_unique(legacy_database):
    connection, config = legacy_database
    original_rows = device_rows(connection)
    command.upgrade(config, "head")
    command.downgrade(config, "0003_sync_token_expires_at")
    assert device_rows(connection) == original_rows
    assert sa.inspect(connection).get_pk_constraint("sync_device")["constrained_columns"] == ["id"]
    command.upgrade(config, "head")
    assert device_rows(connection) == original_rows


def test_device_migration_downgrade_refuses_to_discard_duplicate_ids(legacy_database):
    connection, config = legacy_database
    command.upgrade(config, "head")
    connection.execute(sa.text(
        "INSERT INTO sync_device SELECT id, 'y', name, created_at, last_seen_at, revoked_at "
        "FROM sync_device WHERE account_id = 'x'"
    ))
    connection.commit()
    original_rows = device_rows(connection)
    with pytest.raises(RuntimeError, match="registered in multiple accounts"):
        command.downgrade(config, "0003_sync_token_expires_at")
    assert device_rows(connection) == original_rows
    assert sa.inspect(connection).get_pk_constraint("sync_device")["constrained_columns"] == ["account_id", "id"]
    assert connection.execute(sa.text("SELECT version_num FROM alembic_version")).scalar_one() == "0004_sync_device_account_key"


def test_device_migration_matches_fresh_metadata():
    engine = sa.create_engine("sqlite:///:memory:")
    with engine.connect() as connection:
        command.upgrade(migration_config(connection), "head")
        migrated = sa.inspect(connection).get_pk_constraint("sync_device")
    engine.dispose()
    engine = sa.create_engine("sqlite:///:memory:")
    Base.metadata.create_all(engine)
    assert sa.inspect(engine).get_pk_constraint("sync_device") == migrated
    engine.dispose()
