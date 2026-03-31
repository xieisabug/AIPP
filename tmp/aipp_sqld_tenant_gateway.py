#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import http.client
import json
import logging
import re
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit
import hmac


LOG = logging.getLogger("aipp_sqld_tenant_gateway")
UUID_RE = re.compile(
    r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$"
)
HOP_BY_HOP_HEADERS = {
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
}


def load_config(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError(f"config at {path} is not a JSON object")

    tenants = data.get("tenants")
    if not isinstance(tenants, dict):
        raise ValueError("config.tenants must be an object")

    path_prefix = str(data.get("path_prefix", "t")).strip().strip("/")
    if not path_prefix:
        raise ValueError("config.path_prefix cannot be empty")

    sqld_url = str(data.get("sqld_url", "")).rstrip("/")
    sqld_auth_token = str(data.get("sqld_auth_token", "")).strip()
    if not sqld_url:
        raise ValueError("config.sqld_url is required")
    if not sqld_auth_token:
        raise ValueError("config.sqld_auth_token is required")

    data["path_prefix"] = path_prefix
    data["sqld_url"] = sqld_url
    data["sqld_auth_token"] = sqld_auth_token
    return data


def sha256_hex(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


class TenantGatewayHandler(BaseHTTPRequestHandler):
    config_path: Path
    server_version = "AippSqldTenantGateway/1.0"
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:  # noqa: N802
        self.handle_request()

    def do_HEAD(self) -> None:  # noqa: N802
        self.handle_request()

    def do_POST(self) -> None:  # noqa: N802
        self.handle_request()

    def do_PUT(self) -> None:  # noqa: N802
        self.handle_request()

    def do_PATCH(self) -> None:  # noqa: N802
        self.handle_request()

    def do_DELETE(self) -> None:  # noqa: N802
        self.handle_request()

    def do_OPTIONS(self) -> None:  # noqa: N802
        self.handle_request()

    def log_message(self, fmt: str, *args: object) -> None:
        LOG.info("%s - %s", self.address_string(), fmt % args)

    def respond_json(self, status: int, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)
        self.close_connection = True

    def respond_text(self, status: int, body: str) -> None:
        encoded = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Connection", "close")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(encoded)
        self.close_connection = True

    def handle_request(self) -> None:
        split = urlsplit(self.path)

        if split.path in {"/", "/healthz"}:
            self.respond_text(200, "ok\n")
            return

        try:
            config = load_config(self.config_path)
        except Exception as exc:  # noqa: BLE001
            LOG.exception("failed to load config")
            self.respond_json(500, {"error": "gateway config error", "detail": str(exc)})
            return

        match = self.match_tenant_path(split.path, config["path_prefix"])
        if not match:
            self.respond_json(404, {"error": "route not found"})
            return

        tenant_id, db_name, rest_path = match
        tenant = config["tenants"].get(tenant_id)
        if tenant is None:
            self.respond_json(404, {"error": "tenant not found"})
            return

        if not self.is_authorized(tenant):
            self.respond_json(401, {"error": "unauthorized"})
            return

        namespace_prefix = str(tenant.get("namespace_prefix") or tenant_id)
        namespace = f"{namespace_prefix}-{db_name}"
        upstream_path = rest_path or "/"
        if split.query:
            upstream_path = f"{upstream_path}?{split.query}"

        body = self.read_body()

        try:
            status, reason, headers, response_body = self.forward_request(
                sqld_url=config["sqld_url"],
                sqld_auth_token=config["sqld_auth_token"],
                namespace=namespace,
                upstream_path=upstream_path,
                body=body,
            )
        except Exception as exc:  # noqa: BLE001
            LOG.exception("upstream proxy failed")
            self.respond_json(502, {"error": "upstream request failed", "detail": str(exc)})
            return

        self.send_response(status, reason)
        for header, value in headers:
            lower = header.lower()
            if lower in HOP_BY_HOP_HEADERS or lower == "content-length":
                continue
            self.send_header(header, value)
        self.send_header("Content-Length", str(len(response_body)))
        self.send_header("Connection", "close")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(response_body)
        self.close_connection = True

    @staticmethod
    def match_tenant_path(path: str, path_prefix: str) -> tuple[str, str, str] | None:
        pattern = rf"^/{re.escape(path_prefix)}/(?P<tenant_id>[^/]+)/dev/(?P<db_name>[^/]+)(?P<rest>/.*)?$"
        match = re.match(pattern, path)
        if not match:
            return None

        tenant_id = match.group("tenant_id")
        if not UUID_RE.match(tenant_id):
            return None

        db_name = match.group("db_name")
        rest = match.group("rest") or "/"
        return tenant_id, db_name, rest

    def is_authorized(self, tenant: dict[str, Any]) -> bool:
        header = self.headers.get("Authorization", "")
        if not header.startswith("Bearer "):
            return False

        provided = header[len("Bearer ") :].strip()
        expected = str(tenant.get("token_sha256", ""))
        if not provided or not expected:
            return False

        return hmac.compare_digest(sha256_hex(provided), expected)

    def read_body(self) -> bytes | None:
        length_header = self.headers.get("Content-Length")
        if not length_header:
            return None

        length = int(length_header)
        if length <= 0:
            return None
        return self.rfile.read(length)

    def forward_request(
        self,
        *,
        sqld_url: str,
        sqld_auth_token: str,
        namespace: str,
        upstream_path: str,
        body: bytes | None,
    ) -> tuple[int, str, list[tuple[str, str]], bytes]:
        split = urlsplit(sqld_url)
        if split.scheme not in {"http", "https"}:
            raise ValueError(f"unsupported sqld URL scheme: {split.scheme}")

        connection_class = http.client.HTTPSConnection if split.scheme == "https" else http.client.HTTPConnection
        port = split.port or (443 if split.scheme == "https" else 80)
        connection = connection_class(split.hostname, port, timeout=60)

        upstream_base_path = split.path.rstrip("/")
        full_path = f"{upstream_base_path}{upstream_path}"

        forwarded_headers: dict[str, str] = {}
        for header, value in self.headers.items():
            lower = header.lower()
            if lower in HOP_BY_HOP_HEADERS or lower in {"host", "authorization", "x-namespace", "content-length"}:
                continue
            forwarded_headers[header] = value

        forwarded_headers["Authorization"] = f"Bearer {sqld_auth_token}"
        forwarded_headers["x-namespace"] = namespace
        forwarded_headers["Host"] = split.hostname or "127.0.0.1"
        forwarded_headers["X-Forwarded-For"] = self.client_address[0]
        forwarded_headers["X-AIPP-Tenant-Namespace"] = namespace

        connection.request(self.command, full_path, body=body, headers=forwarded_headers)
        response = connection.getresponse()
        try:
            response_body = response.read()
            headers = [(header, value) for header, value in response.getheaders()]
            return response.status, response.reason, headers, response_body
        finally:
            response.close()
            connection.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="AIPP sqld tenant gateway")
    parser.add_argument("--config", required=True, help="Path to the gateway JSON config file")
    parser.add_argument("--bind", default="0.0.0.0", help="Bind address")
    parser.add_argument("--port", type=int, default=9000, help="Listen port")
    parser.add_argument("--log-level", default="INFO", help="Python logging level")
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    logging.basicConfig(
        level=getattr(logging, args.log_level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

    handler = TenantGatewayHandler
    handler.config_path = Path(args.config)
    server = ThreadingHTTPServer((args.bind, args.port), handler)
    LOG.info("Starting gateway on %s:%s using config %s", args.bind, args.port, handler.config_path)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        LOG.info("Received interrupt, shutting down")
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
