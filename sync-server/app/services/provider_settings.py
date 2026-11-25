"""Provider selections share their model code and one account-scoped provider reference."""
from __future__ import annotations

import re

from sqlalchemy import func, select
from sqlalchemy.orm import Session

from app.models import SyncChange, SyncObject

REFERENCE = "value_provider_id"
CAPABILITY_HEADER = "X-AIPP-Provider-Refs"
UPGRADE_REQUIRED = "模型配置使用全局供应商引用，请升级 AIPP 客户端后重新同步"
SUMMARY_MODELS = {"title_model", "form_autofill_model", "memory_summary_model",
                  "auto_review_model", "conversation_summary_model",
                  "default_light_model", "default_decision_model"}
SUMMARY_PROVIDERS = {key.removesuffix("_model") + "_provider_id" for key in SUMMARY_MODELS} | {"provider_id"}
EXPERIMENTAL_MODELS = {"butler_model_id", "butler_helper_model_id", "butler_group_dispatch_model_id",
                       "mcp_summarizer_model_id", "assistant_summarizer_model_id",
                       "conversation_summary_model"}


def selection(kind, payload):
    """Return (model prefix, provider slot) only for known embedded references."""
    fields = (payload or {}).get("fields", {})
    value = fields.get("value")
    if not isinstance(value, str):
        return None
    feature, key = fields.get("feature_code"), fields.get("key")
    if kind == "assistant.model_config":
        if fields.get("name") != "acp_provider":
            return None
        combined = False
    elif kind == "system.feature_config" and isinstance(key, str):
        if feature == "experimental":
            combined = key in EXPERIMENTAL_MODELS
            if not combined and key != "conversation_summary_provider_id":
                return None
        elif feature == "conversation_summary":
            combined = key in SUMMARY_MODELS
            if not combined and key not in SUMMARY_PROVIDERS:
                return None
        else:
            return None
    else:
        return None
    prefix, sep, slot = value.strip().rpartition("%%") if combined else ("", "", value.strip())
    if combined and not sep:
        return None
    if slot != "@ref":
        digits = slot.removeprefix("+").lstrip("0")
        if not re.fullmatch(r"[0-9]{1,19}", digits) or int(digits) > 2**63 - 1:
            return None
        slot = digits
    return prefix + sep, slot


def canonical_payload(kind, payload):
    parts = selection(kind, payload)
    if parts is None:
        return payload
    ref = payload.get("refs", {}).get(REFERENCE)
    if not isinstance(ref, str) or not re.fullmatch(r"[A-Za-z0-9._:+/=\-]{1,128}", ref):
        raise ValueError("provider_sync_id_required")
    return {**payload, "fields": {**payload["fields"], "value": parts[0] + "@ref"}}


def requires_ref_reader(kind, payload):
    parts = selection(kind, payload)
    return parts is not None and parts[1] == "@ref"


def repair_payload(db: Session, current: SyncObject):
    """Find an unambiguous historical mapping emitted by this account and source device."""
    payload = current.payload_json
    parts = selection(current.object_type, payload)
    if parts is None:
        return None
    if REFERENCE in payload.get("refs", {}):
        try:
            return canonical_payload(current.object_type, payload)
        except ValueError:
            return None
    if parts[1] == "@ref":
        return None
    sequence = db.scalar(select(SyncChange.seq).where(
        SyncChange.account_id == current.account_id, SyncChange.object_type == current.object_type,
        SyncChange.object_id == current.object_id, SyncChange.version == current.version,
        SyncChange.device_id == current.updated_by_device_id,
    ))
    if sequence is None:
        return None
    # Structural FKs carry both the writer's row number and the global ID. Require
    # a witness at or before the legacy version and reject any contradictory history.
    identities = set()
    witnessed = False
    for kinds, column in [(("llm.model", "llm.provider_config", "llm.provider_secret", "llm.model_request_mode_preference"), "llm_provider_id"),
                          (("assistant.model",), "provider_id")]:
        ref = SyncChange.payload_json["refs"][column].as_string()
        rows = db.execute(select(ref, func.min(SyncChange.seq)).where(
            SyncChange.account_id == current.account_id,
            SyncChange.device_id == current.updated_by_device_id,
            SyncChange.object_type.in_(kinds), SyncChange.operation == "upsert",
            SyncChange.payload_json["fields"][column].as_integer() == int(parts[1]),
            ref.is_not(None),
        ).group_by(ref))
        for identity, first_seq in rows:
            identities.add(identity)
            witnessed |= first_seq <= sequence
    # Older reference-aware writers also sent the original row number in settings.
    # A disagreement here rules out numeric-ID reuse even if structural rows agree.
    settings = db.execute(select(SyncChange.object_type, SyncChange.payload_json, SyncChange.seq).where(
        SyncChange.account_id == current.account_id, SyncChange.device_id == current.updated_by_device_id,
        SyncChange.object_type.in_(("system.feature_config", "assistant.model_config")),
        SyncChange.operation == "upsert", SyncChange.payload_json["refs"][REFERENCE].as_string().is_not(None),
    ))
    for kind, historical, seq in settings:
        other = selection(kind, historical)
        if other is not None and other[1] == parts[1]:
            identity = historical["refs"][REFERENCE]
            if not isinstance(identity, str):
                return None
            identities.add(identity)
            witnessed |= seq <= sequence
    if not witnessed or len(identities) != 1:
        return None
    identity = identities.pop()
    provider = db.get(SyncObject, (current.account_id, "llm.provider", identity))
    if provider is None or provider.deleted_at is not None:
        return None
    return canonical_payload(current.object_type, {
        **payload, "refs": {**payload.get("refs", {}), REFERENCE: identity},
    })
