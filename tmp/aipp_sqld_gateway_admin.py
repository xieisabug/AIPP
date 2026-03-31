#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import json
import secrets
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


DEFAULT_PATH_PREFIX = "t"


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def load_json(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError(f"config at {path} is not a JSON object")
    return data


def save_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_suffix(path.suffix + ".tmp")
    tmp_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    tmp_path.replace(path)
    path.chmod(0o600)


def normalize_path_prefix(value: str) -> str:
    normalized = value.strip().strip("/")
    if not normalized:
        raise ValueError("path prefix cannot be empty")
    if any(ch not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-" for ch in normalized):
        raise ValueError("path prefix may only contain letters, digits, underscore and hyphen")
    return normalized


def normalize_tenant_id(value: str) -> str:
    return str(uuid.UUID(value))


def token_sha256(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


def load_config(path: Path) -> dict[str, Any]:
    if not path.exists():
        raise FileNotFoundError(f"config not found: {path}")

    config = load_json(path)
    config.setdefault("version", 1)
    config.setdefault("path_prefix", DEFAULT_PATH_PREFIX)
    tenants = config.setdefault("tenants", {})
    if not isinstance(tenants, dict):
        raise ValueError("config.tenants must be an object")

    return config


def ensure_sqld_token(args: argparse.Namespace) -> str:
    if args.sqld_token:
        return args.sqld_token
    if args.sqld_token_file:
        return Path(args.sqld_token_file).read_text(encoding="utf-8").strip()
    raise ValueError("either --sqld-token or --sqld-token-file is required")


def cmd_init(args: argparse.Namespace) -> int:
    config_path = Path(args.config)
    if config_path.exists():
        config = load_config(config_path)
    else:
        config = {"version": 1, "tenants": {}}

    config["path_prefix"] = normalize_path_prefix(args.path_prefix)
    config["sqld_url"] = args.sqld_url.rstrip("/")
    config["sqld_auth_token"] = ensure_sqld_token(args)
    config["updated_at"] = utc_now_iso()
    config.setdefault("created_at", config["updated_at"])

    save_json(config_path, config)
    print(json.dumps({"config": str(config_path), "tenant_count": len(config["tenants"])}, indent=2))
    return 0


def write_credentials_file(
    credentials_path: Path,
    tenant_id: str,
    path_prefix: str,
    namespace_prefix: str,
    access_token: str,
) -> None:
    payload = {
        "tenant_id": tenant_id,
        "path_prefix": path_prefix,
        "base_path": f"/{path_prefix}/{tenant_id}",
        "namespace_prefix": namespace_prefix,
        "access_token": access_token,
        "issued_at": utc_now_iso(),
    }
    save_json(credentials_path, payload)


def cmd_add_tenant(args: argparse.Namespace) -> int:
    config_path = Path(args.config)
    config = load_config(config_path)

    tenant_id = normalize_tenant_id(args.tenant_id) if args.tenant_id else str(uuid.uuid4())
    namespace_prefix = tenant_id
    tenants: dict[str, Any] = config["tenants"]
    existing = tenants.get(tenant_id)
    credentials_dir = Path(args.credentials_dir) if args.credentials_dir else None
    credentials_path = credentials_dir / f"{tenant_id}.json" if credentials_dir else None

    if existing and not args.rotate_token:
        result = {
            "tenant_id": tenant_id,
            "created": False,
            "rotated": False,
            "credentials_file": str(credentials_path) if credentials_path else None,
            "base_path": f"/{config['path_prefix']}/{tenant_id}",
        }
        print(json.dumps(result, indent=2))
        return 0

    access_token = args.token or secrets.token_urlsafe(32)
    description = args.description if args.description is not None else (existing.get("description") if existing else None)
    tenants[tenant_id] = {
        "tenant_id": tenant_id,
        "namespace_prefix": namespace_prefix,
        "token_sha256": token_sha256(access_token),
        "created_at": existing.get("created_at", utc_now_iso()) if existing else utc_now_iso(),
        "updated_at": utc_now_iso(),
        "description": description,
    }
    config["updated_at"] = utc_now_iso()
    save_json(config_path, config)

    if credentials_path is not None:
        credentials_path.parent.mkdir(parents=True, exist_ok=True)
        write_credentials_file(
            credentials_path=credentials_path,
            tenant_id=tenant_id,
            path_prefix=config["path_prefix"],
            namespace_prefix=namespace_prefix,
            access_token=access_token,
        )

    result = {
        "tenant_id": tenant_id,
        "created": existing is None,
        "rotated": existing is not None,
        "credentials_file": str(credentials_path) if credentials_path else None,
        "base_path": f"/{config['path_prefix']}/{tenant_id}",
        "access_token": access_token,
    }
    print(json.dumps(result, indent=2))
    return 0


def cmd_list_tenants(args: argparse.Namespace) -> int:
    config = load_config(Path(args.config))
    tenant_ids = sorted(config["tenants"].keys())

    if args.ids_only:
        for tenant_id in tenant_ids:
            print(tenant_id)
        return 0

    payload = {
        "path_prefix": config["path_prefix"],
        "tenant_count": len(tenant_ids),
        "tenants": [
            {
                "tenant_id": tenant_id,
                "namespace_prefix": config["tenants"][tenant_id].get("namespace_prefix", tenant_id),
                "created_at": config["tenants"][tenant_id].get("created_at"),
                "updated_at": config["tenants"][tenant_id].get("updated_at"),
            }
            for tenant_id in tenant_ids
        ],
    }
    print(json.dumps(payload, indent=2))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Manage AIPP sqld tenant gateway configuration.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    init_parser = subparsers.add_parser("init", help="Create or update the gateway config.")
    init_parser.add_argument("--config", required=True)
    init_parser.add_argument("--sqld-url", required=True)
    init_parser.add_argument("--sqld-token")
    init_parser.add_argument("--sqld-token-file")
    init_parser.add_argument("--path-prefix", default=DEFAULT_PATH_PREFIX)
    init_parser.set_defaults(func=cmd_init)

    add_parser = subparsers.add_parser("add-tenant", help="Create or rotate a tenant credential.")
    add_parser.add_argument("--config", required=True)
    add_parser.add_argument("--tenant-id")
    add_parser.add_argument("--token")
    add_parser.add_argument("--credentials-dir")
    add_parser.add_argument("--description")
    add_parser.add_argument("--rotate-token", action="store_true")
    add_parser.set_defaults(func=cmd_add_tenant)

    list_parser = subparsers.add_parser("list-tenants", help="List configured tenants.")
    list_parser.add_argument("--config", required=True)
    list_parser.add_argument("--ids-only", action="store_true")
    list_parser.set_defaults(func=cmd_list_tenants)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    try:
        return args.func(args)
    except Exception as exc:  # noqa: BLE001
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
