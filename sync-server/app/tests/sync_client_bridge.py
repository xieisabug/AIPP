"""JSON-lines bridge: real FastAPI routes + shared in-memory server DB.

Used by the opt-in Rust client/server integration test. No network, live config,
business database, or alternative implementation of the client apply algorithm.
"""
from __future__ import annotations

import json
import sys

from fastapi.testclient import TestClient

from app.config import Settings
from app.factory import create_app


def main() -> None:
    app = create_app(Settings(
        _env_file=None, database_url="sqlite:///:memory:",
        bootstrap_token="integration-token", bootstrap_account_id="integration",
        base_url="http://localhost", token_ttl_days=0,
    ))
    with TestClient(app) as client:
        for line in sys.stdin:
            request = json.loads(line)
            headers = {"Authorization": "Bearer integration-token",
                       "X-AIPP-Device-ID": request.get("device", "device-b")}
            response = client.request(request.get("method", "GET"), request["path"],
                                      headers=headers, json=request.get("body"))
            print(json.dumps({"status": response.status_code, "body": response.json()}), flush=True)


if __name__ == "__main__":
    main()
