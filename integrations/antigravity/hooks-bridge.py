#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import secrets
import selectors
import signal
import socket
import sys
import time
import uuid

VALID_EVENTS = (
    "SessionStart",
    "PreInvocation",
    "PostInvocation",
    "PreToolUse",
    "PostToolUse",
    "Stop",
)

FORBIDDEN_WIRE_KEYS = {
    "transcriptPath",
    "artifactDirectoryPath",
    "lastUserInput",
    "args",
    "csrfToken",
    "oauthToken",
    "sidecarUiToken",
    "apiKey",
}

VALID_PHASES = (
    "idle",
    "running",
    "runningTool",
    "backgroundActive",
    "completed",
    "failed",
    "canceled",
)

CATALOG_NAV_SCHEMA_VERSION = "antigravity-catalog-nav-v1"

DEFAULT_CAPABILITIES = {
    "bundleId": "com.google.antigravity",
    "installedVersion": "2.29.0",
    "languageServerCl": "1013060670",
    "agentapiCommands": [
        "get-conversation-metadata",
        "new-conversation",
        "send-message",
    ],
    "sidecarSdkVersion": "1.0.0",
    "hookEvents": list(VALID_EVENTS),
    "supportsSessionCatalog": True,
    "supportsTargetFocus": True,
    "supportsStopTurn": False,
    "supportsApprovalGate": False,
    "schemaVerified": True,
}


def valid_id(value, limit=4096):
    return isinstance(value, str) and 0 < len(value.encode("utf-8")) <= limit


def canonical_event_key(conversation_id, workspace_path, event, prior=None):
    explicit = event.get("eventId")
    if isinstance(explicit, str) and explicit:
        return f"{conversation_id}:{workspace_path}:{explicit}"
    event_type = event["eventType"]
    inv = event.get("invocationNum")
    step = event.get("stepIdx")
    exec_num = event.get("executionNum")
    inv_part = (
        inv
        if isinstance(inv, int) and not isinstance(inv, bool)
        else (prior["invocationNum"] if prior else -1)
    )
    step_part = (
        step
        if isinstance(step, int) and not isinstance(step, bool)
        else (prior["stepIdx"] if prior else -1)
    )
    exec_part = (
        exec_num
        if isinstance(exec_num, int) and not isinstance(exec_num, bool)
        else (prior["executionNum"] if prior else -1)
    )
    tool_part = event.get("toolName") or ""
    term_part = event.get("terminationReason") or ""
    err_part = "1" if event.get("hasError") else "0"
    fully_idle = event.get("fullyIdle")
    idle_part = "1" if fully_idle is True else ("0" if fully_idle is False else "-")
    return (
        f"{conversation_id}:{workspace_path}:{event_type}:"
        f"{inv_part}:{step_part}:{exec_part}:{tool_part}:{term_part}:{err_part}:{idle_part}"
    )


def reduce_row(prior, event):
    conversation_id = event["conversationId"]
    workspace_path = event["workspacePath"]
    event_type = event["eventType"]
    title = event.get("title") or (prior.get("title") if prior else None) or conversation_id
    project_id = event.get("projectId") if isinstance(event.get("projectId"), str) else (prior.get("projectId") if prior else "")
    model_name = event.get("modelName") or (prior["modelName"] if prior else "")
    inv = event.get("invocationNum")
    if not (isinstance(inv, int) and not isinstance(inv, bool)):
        inv = prior["invocationNum"] if prior else 0
    step = event.get("stepIdx")
    if not (isinstance(step, int) and not isinstance(step, bool)):
        step = prior["stepIdx"] if prior else 0
    exec_num = event.get("executionNum")
    if not (isinstance(exec_num, int) and not isinstance(exec_num, bool)):
        exec_num = prior["executionNum"] if prior else 0
    tool_name = event.get("toolName")
    has_error = bool(event.get("hasError", False))
    term_reason = event.get("terminationReason")
    fully_idle = event.get("fullyIdle")

    if event_type == "SessionStart":
        phase = "idle"
        last_tool = None
        term_out = None
        idle_out = True
    elif event_type == "PreInvocation":
        phase = "running"
        last_tool = prior["lastToolName"] if prior else None
        term_out = None
        idle_out = False
    elif event_type == "PreToolUse":
        phase = "runningTool"
        last_tool = tool_name if isinstance(tool_name, str) and tool_name else (prior["lastToolName"] if prior else None)
        term_out = None
        idle_out = False
    elif event_type == "PostToolUse":
        phase = "running"
        last_tool = tool_name if isinstance(tool_name, str) and tool_name else (prior["lastToolName"] if prior else None)
        term_out = None
        idle_out = False
    elif event_type == "PostInvocation":
        phase = "failed" if has_error else "running"
        last_tool = prior["lastToolName"] if prior else None
        term_out = "ERROR" if has_error else None
        idle_out = False
    elif event_type == "Stop":
        last_tool = prior["lastToolName"] if prior else None
        term_out = term_reason if isinstance(term_reason, str) and term_reason else None
        if term_out == "ERROR" or has_error:
            phase = "failed"
            idle_out = bool(fully_idle) if isinstance(fully_idle, bool) else False
        elif term_out == "USER_CANCELED":
            phase = "canceled"
            idle_out = bool(fully_idle) if isinstance(fully_idle, bool) else False
        elif fully_idle is True and term_out == "NO_TOOL_CALL":
            phase = "completed"
            idle_out = True
        else:
            phase = "backgroundActive"
            idle_out = False
    else:
        raise ValueError(f"unsupported eventType: {event_type}")

    transitions = (prior["transitionCount"] if prior else 0) + 1
    return {
        "conversationId": conversation_id,
        "workspacePath": workspace_path,
        "title": title,
        "projectId": project_id,
        "modelName": model_name,
        "phase": phase,
        "lastEventType": event_type,
        "invocationNum": max(0, int(inv)),
        "stepIdx": max(0, int(step)),
        "executionNum": max(0, int(exec_num)),
        "lastToolName": last_tool,
        "terminationReason": term_out,
        "fullyIdle": idle_out,
        "transitionCount": transitions,
    }


def normalize_workspace_uri(raw_value):
    if not isinstance(raw_value, str):
        return None
    if raw_value.startswith("file://"):
        return raw_value[len("file://") :]
    return raw_value


def sanitize_catalog_entry(raw_entry):
    if not isinstance(raw_entry, dict):
        raise ValueError("catalog entry must be a JSON object")
    conversation_id = raw_entry.get("conversationId") or raw_entry.get("cascadeId") or raw_entry.get("trajectoryId")
    if not valid_id(conversation_id, 256):
        raise ValueError("missing or invalid conversationId in catalog entry")
    workspace_path = normalize_workspace_uri(raw_entry.get("workspacePath"))
    if not valid_id(workspace_path, 4096):
        workspaces = raw_entry.get("workspacePaths") or raw_entry.get("workspaceUris")
        if isinstance(workspaces, list) and workspaces:
            workspace_path = normalize_workspace_uri(workspaces[0])
        elif isinstance(raw_entry.get("workspaces"), list) and raw_entry["workspaces"]:
            first_ws = raw_entry["workspaces"][0]
            if isinstance(first_ws, dict):
                workspace_path = normalize_workspace_uri(first_ws.get("workspaceFolderAbsoluteUri"))
    if not valid_id(workspace_path, 4096):
        raise ValueError("missing or invalid workspacePath in catalog entry")

    raw_title = raw_entry.get("title") or raw_entry.get("summary")
    if not valid_id(raw_title, 512):
        annotations = raw_entry.get("annotations")
        if isinstance(annotations, dict) and valid_id(annotations.get("title"), 512):
            raw_title = annotations["title"]
        else:
            raw_title = conversation_id

    raw_project = raw_entry.get("projectId") or ""
    if not isinstance(raw_project, str) or len(raw_project.encode("utf-8")) > 256:
        raw_project = ""

    raw_model = raw_entry.get("modelName") if valid_id(raw_entry.get("modelName"), 256) else ""
    phase = raw_entry.get("phase") if raw_entry.get("phase") in VALID_PHASES else "idle"
    last_event = raw_entry.get("lastEventType") if raw_entry.get("lastEventType") in VALID_EVENTS else "SessionStart"
    fully_idle = raw_entry.get("fullyIdle")
    if not isinstance(fully_idle, bool):
        fully_idle = phase in ("idle", "completed")

    inv = raw_entry.get("invocationNum")
    step = raw_entry.get("stepIdx") if "stepIdx" in raw_entry else raw_entry.get("stepCount")
    exec_num = raw_entry.get("executionNum")
    transitions = raw_entry.get("transitionCount")

    return {
        "conversationId": conversation_id,
        "workspacePath": workspace_path,
        "title": raw_title,
        "projectId": raw_project,
        "modelName": raw_model,
        "phase": phase,
        "lastEventType": last_event,
        "invocationNum": inv if isinstance(inv, int) and not isinstance(inv, bool) and inv >= 0 else 0,
        "stepIdx": step if isinstance(step, int) and not isinstance(step, bool) and step >= 0 else 0,
        "executionNum": exec_num if isinstance(exec_num, int) and not isinstance(exec_num, bool) and exec_num >= 0 else 0,
        "lastToolName": raw_entry.get("lastToolName") if valid_id(raw_entry.get("lastToolName"), 256) else None,
        "terminationReason": raw_entry.get("terminationReason") if valid_id(raw_entry.get("terminationReason"), 128) else None,
        "fullyIdle": fully_idle,
        "transitionCount": transitions if isinstance(transitions, int) and not isinstance(transitions, bool) and transitions >= 1 else 1,
    }


def sanitize_hook_stdin(raw_payload, event_type):
    if not isinstance(raw_payload, dict):
        raise ValueError("hook stdin payload must be a JSON object")
    if event_type not in VALID_EVENTS:
        raise ValueError(f"invalid hook event type: {event_type}")
    conversation_id = raw_payload.get("conversationId")
    if not valid_id(conversation_id, 256):
        raise ValueError("missing or invalid conversationId")
    workspace_path = raw_payload.get("workspacePath")
    if not valid_id(workspace_path, 4096):
        workspaces = raw_payload.get("workspacePaths")
        if isinstance(workspaces, list) and workspaces and valid_id(workspaces[0], 4096):
            workspace_path = workspaces[0]
        else:
            raise ValueError("missing or invalid workspacePaths")

    tool_call = raw_payload.get("toolCall")
    tool_name = None
    if isinstance(tool_call, dict) and valid_id(tool_call.get("name"), 256):
        tool_name = tool_call["name"]
    elif valid_id(raw_payload.get("toolName"), 256):
        tool_name = raw_payload["toolName"]

    sanitized = {
        "conversationId": conversation_id,
        "workspacePath": workspace_path,
        "eventType": event_type,
        "modelName": raw_payload.get("modelName") if valid_id(raw_payload.get("modelName"), 256) else "",
        "hasError": bool(raw_payload.get("error")),
    }
    if valid_id(raw_payload.get("title"), 512):
        sanitized["title"] = raw_payload["title"]
    if isinstance(raw_payload.get("projectId"), str) and len(raw_payload["projectId"].encode("utf-8")) <= 256:
        sanitized["projectId"] = raw_payload["projectId"]
    for int_key in ("invocationNum", "stepIdx", "executionNum"):
        val = raw_payload.get(int_key)
        if isinstance(val, int) and not isinstance(val, bool) and 0 <= val <= 1_000_000_000:
            sanitized[int_key] = val
    if tool_name is not None:
        sanitized["toolName"] = tool_name
    term_reason = raw_payload.get("terminationReason")
    if valid_id(term_reason, 128):
        sanitized["terminationReason"] = term_reason
    fully_idle = raw_payload.get("fullyIdle")
    if isinstance(fully_idle, bool):
        sanitized["fullyIdle"] = fully_idle
    event_id = raw_payload.get("eventId")
    if valid_id(event_id, 256):
        sanitized["eventId"] = event_id
    return sanitized


def send_uds_json(socket_path, request, timeout=2.0):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(timeout)
        sock.connect(str(socket_path))
        sock.sendall(json.dumps(request, separators=(",", ":")).encode("utf-8") + b"\n")
        buf = b""
        while b"\n" not in buf:
            chunk = sock.recv(65536)
            if not chunk or len(buf) + len(chunk) > 1_048_576:
                raise ValueError("incomplete or oversized UDS reply")
            buf += chunk
        line = buf.split(b"\n", 1)[0]
        return json.loads(line.decode("utf-8"))


def contains_forbidden_keys(obj):
    if isinstance(obj, dict):
        if FORBIDDEN_WIRE_KEYS.intersection(obj.keys()):
            return True
        return any(contains_forbidden_keys(v) for v in obj.values())
    if isinstance(obj, list):
        return any(contains_forbidden_keys(item) for item in obj)
    return False


class BridgeDaemon:
    def __init__(self, directory, instance_id, generation, workspaces, ttl, heartbeat_ttl, schema_verified=True):
        self.directory = Path(directory).resolve()
        self.instance_id = instance_id
        self.generation = generation
        self.allowed_workspaces = list(dict.fromkeys(workspaces))
        self.ttl = ttl
        self.heartbeat_ttl = heartbeat_ttl
        self.token = secrets.token_hex(32)
        self.schema_verified = bool(schema_verified)
        self.capabilities = dict(DEFAULT_CAPABILITIES)
        self.capabilities["schemaVerified"] = self.schema_verified
        self.capabilities["supportsSessionCatalog"] = self.schema_verified
        self.capabilities["supportsTargetFocus"] = self.schema_verified

        self.subscription_id = None
        self.cursor = 0
        self.min_cursor = 0
        self.rows = {}
        self.seen_keys = {}
        self.changes = []
        self.selected = None
        self.select_override = None
        self.stream_healthy = True
        self.forced_gap = False
        self.last_heartbeat = time.monotonic()

        self.metadata_reads = 0
        self.ingested_events = 0
        self.duplicate_events_dropped = 0
        self.unauthorized_rejections = 0
        self.transcript_reads = 0
        self.token_reads = 0
        self.open_requests = 0
        self.open_confirmed = 0
        self.close_count = 0
        self.running = True

    def is_stream_healthy(self):
        if not self.stream_healthy:
            return False
        if self.heartbeat_ttl is not None and (time.monotonic() - self.last_heartbeat) > self.heartbeat_ttl:
            return False
        return True

    def selected_payload(self):
        if self.selected is not None and self.selected in self.rows:
            return {
                "conversationId": self.selected[0],
                "workspacePath": self.selected[1],
            }
        return None

    def set_schema_verified(self, verified):
        self.schema_verified = bool(verified)
        self.capabilities["schemaVerified"] = self.schema_verified
        self.capabilities["supportsSessionCatalog"] = self.schema_verified
        self.capabilities["supportsTargetFocus"] = self.schema_verified

    def seed_catalog_entries(self, raw_entries, initial_selected=None):
        if not isinstance(raw_entries, list):
            raise ValueError("catalog entries must be a list")
        seeded = 0
        for raw in raw_entries:
            row = sanitize_catalog_entry(raw)
            conv_id = row["conversationId"]
            ws_path = row["workspacePath"]
            if ws_path not in self.allowed_workspaces:
                raise ValueError(f"workspacePath not allowed: {ws_path}")
            session_key = (conv_id, ws_path)
            self.rows[session_key] = row
            self.cursor += 1
            ev_key = f"{conv_id}:{ws_path}:catalog:{self.cursor}"
            self.seen_keys.setdefault(session_key, set()).add(ev_key)
            self.changes.append({
                "sequence": self.cursor,
                "eventKey": ev_key,
                "row": row,
            })
            seeded += 1
        if len(self.changes) > 1024:
            self.changes = self.changes[-1024:]
            self.min_cursor = self.changes[0]["sequence"] - 1
        if isinstance(initial_selected, dict):
            sel_conv = initial_selected.get("conversationId")
            sel_ws = initial_selected.get("workspacePath")
            if (sel_conv, sel_ws) in self.rows:
                self.selected = (sel_conv, sel_ws)
        self.last_heartbeat = time.monotonic()
        return seeded

    def base_reply(self, req, status):
        return {
            "version": 1,
            "subscriptionId": req.get("subscriptionId", "") if isinstance(req, dict) else "",
            "id": req.get("id", "") if isinstance(req, dict) else "",
            "instanceId": self.instance_id,
            "generation": self.generation,
            "status": status,
        }

    def sorted_rows(self):
        return [self.rows[k] for k in sorted(self.rows.keys())]

    def handle_main_request(self, req):
        if not isinstance(req, dict) or contains_forbidden_keys(req):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        if req.get("version") != 1 or req.get("token") != self.token:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        if req.get("instanceId") != self.instance_id or req.get("generation") != self.generation:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "stale")

        op = req.get("op")
        if op == "ingest":
            event = req.get("event")
            if not isinstance(event, dict) or contains_forbidden_keys(event):
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            conv_id = event.get("conversationId")
            ws_path = event.get("workspacePath")
            event_type = event.get("eventType")
            if (
                not valid_id(conv_id, 256)
                or not valid_id(ws_path, 4096)
                or ws_path not in self.allowed_workspaces
                or event_type not in VALID_EVENTS
            ):
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            session_key = (conv_id, ws_path)
            seen = self.seen_keys.setdefault(session_key, set())
            prior = self.rows.get(session_key)
            ev_key = canonical_event_key(conv_id, ws_path, event, prior=prior)
            self.last_heartbeat = time.monotonic()
            if ev_key in seen:
                self.duplicate_events_dropped += 1
                reply = self.base_reply(req, "duplicate")
                reply["cursor"] = self.cursor
                return reply
            seen.add(ev_key)
            row = reduce_row(prior, event)
            self.rows[session_key] = row
            self.cursor += 1
            self.ingested_events += 1
            self.changes.append({
                "sequence": self.cursor,
                "eventKey": ev_key,
                "row": row,
            })
            if len(self.changes) > 1024:
                self.changes = self.changes[-1024:]
                self.min_cursor = self.changes[0]["sequence"] - 1
            reply = self.base_reply(req, "ok")
            reply["cursor"] = self.cursor
            return reply

        if op == "seed-catalog":
            if (
                not self.schema_verified
                or req.get("schemaVersion") != CATALOG_NAV_SCHEMA_VERSION
            ):
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            entries = req.get("entries")
            if not isinstance(entries, list) or contains_forbidden_keys(entries):
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            try:
                seeded = self.seed_catalog_entries(entries, initial_selected=req.get("selected"))
            except ValueError:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            reply = self.base_reply(req, "ok")
            reply.update(cursor=self.cursor, seeded=seeded)
            return reply

        sub_id = req.get("subscriptionId")
        req_id = req.get("id")
        if not valid_id(sub_id, 256) or not valid_id(req_id, 256):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")

        if op == "subscribe":
            if req.get("activate") is not False:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            if not self.schema_verified:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            if self.subscription_id is not None and self.subscription_id != sub_id:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            if not self.is_stream_healthy():
                return self.base_reply(req, "stale")
            self.forced_gap = False
            self.subscription_id = sub_id
            self.metadata_reads += 1
            reply = self.base_reply(req, "ok")
            reply.update(
                capabilities=self.capabilities,
                cursor=self.cursor,
                rows=self.sorted_rows(),
                changes=[],
                selected=self.selected_payload(),
                transcriptReads=self.transcript_reads,
                tokenReads=self.token_reads,
            )
            return reply

        if op == "read":
            if self.subscription_id is None or self.subscription_id != sub_id:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            if not self.schema_verified:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            if not self.is_stream_healthy():
                return self.base_reply(req, "stale")
            after = req.get("after")
            if not (isinstance(after, int) and not isinstance(after, bool)) or after < 0:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            if self.forced_gap or after < self.min_cursor or after > self.cursor:
                self.forced_gap = False
                return self.base_reply(req, "gap")
            self.metadata_reads += 1
            delta = [c for c in self.changes if c["sequence"] > after]
            reply = self.base_reply(req, "ok")
            reply.update(
                capabilities=self.capabilities,
                cursor=self.cursor,
                rows=self.sorted_rows(),
                changes=delta,
                selected=self.selected_payload(),
                transcriptReads=self.transcript_reads,
                tokenReads=self.token_reads,
            )
            return reply

        if op == "open":
            if self.subscription_id is None or self.subscription_id != sub_id:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            if (
                not self.schema_verified
                or not self.capabilities.get("supportsSessionCatalog")
                or not self.capabilities.get("supportsTargetFocus")
                or req.get("schemaVersion") != CATALOG_NAV_SCHEMA_VERSION
            ):
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            if not self.is_stream_healthy():
                return self.base_reply(req, "stale")
            target = req.get("target")
            if not isinstance(target, dict) or contains_forbidden_keys(target):
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            conv_id = target.get("conversationId")
            ws_path = target.get("workspacePath")
            if (
                not valid_id(conv_id, 256)
                or not valid_id(ws_path, 4096)
                or ws_path not in self.allowed_workspaces
            ):
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            self.open_requests += 1
            self.metadata_reads += 1
            target_key = (conv_id, ws_path)
            if target_key not in self.rows:
                reply = self.base_reply(req, "unconfirmed")
                reply.update(
                    selected=self.selected_payload(),
                    transcriptReads=self.transcript_reads,
                    tokenReads=self.token_reads,
                )
                return reply
            readback_key = self.select_override if self.select_override is not None else target_key
            if readback_key in self.rows:
                self.selected = readback_key
            if readback_key == target_key and self.selected == target_key:
                self.open_confirmed += 1
                reply = self.base_reply(req, "ok")
            else:
                reply = self.base_reply(req, "unconfirmed")
            reply.update(
                selected=self.selected_payload(),
                transcriptReads=self.transcript_reads,
                tokenReads=self.token_reads,
            )
            return reply

        if op == "close":
            if self.subscription_id is None or self.subscription_id != sub_id:
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            self.subscription_id = None
            self.close_count += 1
            return self.base_reply(req, "ok")

        self.unauthorized_rejections += 1
        return self.base_reply(req, "denied")

    def handle_control_request(self, req):
        op = req.get("op") if isinstance(req, dict) else None
        if op == "stats":
            return {
                "status": "ok",
                "metadataReads": self.metadata_reads,
                "ingestedEvents": self.ingested_events,
                "duplicateEventsDropped": self.duplicate_events_dropped,
                "unauthorizedRejections": self.unauthorized_rejections,
                "transcriptReads": self.transcript_reads,
                "tokenReads": self.token_reads,
                "openRequests": self.open_requests,
                "openConfirmed": self.open_confirmed,
                "selected": self.selected_payload(),
                "schemaVerified": self.schema_verified,
                "closeCount": self.close_count,
                "cursor": self.cursor,
                "sessionCount": len(self.rows),
                "streamHealthy": self.is_stream_healthy(),
                "generation": self.generation,
            }
        if op == "seed-catalog":
            try:
                seeded = self.seed_catalog_entries(req.get("entries", []), initial_selected=req.get("selected"))
                return {"status": "ok", "seeded": seeded, "cursor": self.cursor}
            except Exception as exc:
                return {"status": "error", "message": str(exc)}
        if op == "select-override":
            target = req.get("target")
            if isinstance(target, dict) and valid_id(target.get("conversationId"), 256) and valid_id(target.get("workspacePath"), 4096):
                self.select_override = (target["conversationId"], target["workspacePath"])
            else:
                self.select_override = ("__unselected__", "")
            return {"status": "ok", "selectOverride": list(self.select_override)}
        if op == "clear-select-override":
            self.select_override = None
            return {"status": "ok", "selectOverride": None}
        if op == "delete-session":
            conv_id = req.get("conversationId")
            ws_path = req.get("workspacePath")
            key = (conv_id, ws_path)
            removed = self.rows.pop(key, None) is not None
            if self.selected == key:
                self.selected = None
            if removed:
                self.cursor += 1
            return {"status": "ok", "removed": removed, "cursor": self.cursor}
        if op == "set-schema-verified":
            self.set_schema_verified(bool(req.get("verified", True)))
            return {"status": "ok", "schemaVerified": self.schema_verified}
        if op == "disconnect":
            self.stream_healthy = False
            return {"status": "ok", "streamHealthy": False}
        if op == "expire-heartbeat":
            self.heartbeat_ttl = 0.05
            self.last_heartbeat = time.monotonic() - 10.0
            return {"status": "ok", "streamHealthy": self.is_stream_healthy()}
        if op == "reconnect":
            self.stream_healthy = True
            self.heartbeat_ttl = None
            self.last_heartbeat = time.monotonic()
            return {"status": "ok", "streamHealthy": True}
        if op == "rotate":
            self.generation += 1
            self.subscription_id = None
            return {"status": "ok", "generation": self.generation}
        if op == "gap":
            self.forced_gap = True
            return {"status": "ok", "forcedGap": True}
        if op == "stop-bridge":
            self.running = False
            return {"status": "ok", "stopped": True}
        return {"status": "error", "message": "unknown control op"}


def serve_command(args):
    directory = Path(args.directory).resolve()
    st = os.lstat(directory)
    if st.st_uid != os.getuid() or (st.st_mode & 0o077) != 0:
        raise ValueError("bridge directory must be owned by current user with mode 0700")

    lock_path = directory / "antigravity.lock"
    sock_path = directory / "antigravity.sock"
    ctrl_path = directory / "control.sock"
    grant_path = directory / "antigravity-grant.json"

    if len(str(sock_path).encode("utf-8")) >= 104 or len(str(ctrl_path).encode("utf-8")) >= 104:
        raise ValueError("Unix socket path must be shorter than 104 bytes")

    lock_fd = os.open(str(lock_path), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.write(lock_fd, f"{os.getpid()}\n".encode("utf-8"))

    daemon = BridgeDaemon(
        directory=directory,
        instance_id=args.instance_id,
        generation=args.generation,
        workspaces=args.workspace,
        ttl=args.ttl,
        heartbeat_ttl=args.heartbeat_ttl,
        schema_verified=not args.disable_catalog_schema,
    )
    if args.catalog_file:
        raw_catalog = json.loads(Path(args.catalog_file).read_text(encoding="utf-8"))
        entries = raw_catalog.get("entries", raw_catalog) if isinstance(raw_catalog, dict) else raw_catalog
        selected = raw_catalog.get("selected") if isinstance(raw_catalog, dict) else None
        daemon.seed_catalog_entries(entries, initial_selected=selected)

    main_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    ctrl_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sel = selectors.DefaultSelector()

    def stop_handler(_signum, _frame):
        daemon.running = False

    signal.signal(signal.SIGTERM, stop_handler)
    signal.signal(signal.SIGINT, stop_handler)

    try:
        main_sock.bind(str(sock_path))
        os.chmod(sock_path, 0o600)
        main_sock.listen(16)
        main_sock.setblocking(False)

        ctrl_sock.bind(str(ctrl_path))
        os.chmod(ctrl_path, 0o600)
        ctrl_sock.listen(8)
        ctrl_sock.setblocking(False)

        grant_payload = {
            "version": 1,
            "socketPath": str(sock_path),
            "token": daemon.token,
            "instanceId": daemon.instance_id,
            "generation": daemon.generation,
            "allowedWorkspaces": daemon.allowed_workspaces,
            "capabilities": daemon.capabilities,
        }
        tmp_grant = directory / "antigravity-grant.json.tmp"
        tmp_fd = os.open(str(tmp_grant), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        with os.fdopen(tmp_fd, "w", encoding="utf-8") as handle:
            json.dump(grant_payload, handle, indent=2)
            handle.write("\n")
        os.replace(tmp_grant, grant_path)

        sel.register(main_sock, selectors.EVENT_READ, data="main")
        sel.register(ctrl_sock, selectors.EVENT_READ, data="ctrl")

        started = time.monotonic()
        while daemon.running:
            if daemon.ttl and (time.monotonic() - started) > daemon.ttl:
                break
            events = sel.select(timeout=0.05)
            for key, _ in events:
                listener = key.fileobj
                channel = key.data
                conn, _ = listener.accept()
                with conn:
                    conn.settimeout(2.0)
                    data = b""
                    while b"\n" not in data:
                        chunk = conn.recv(65536)
                        if not chunk or len(data) + len(chunk) > 65536:
                            break
                        data += chunk
                    if not data:
                        continue
                    try:
                        req = json.loads(data.split(b"\n", 1)[0].decode("utf-8"))
                    except Exception:
                        req = None
                    if channel == "main":
                        reply = daemon.handle_main_request(req)
                    else:
                        reply = daemon.handle_control_request(req)
                    conn.sendall(json.dumps(reply, separators=(",", ":")).encode("utf-8") + b"\n")
    finally:
        sel.close()
        try:
            main_sock.close()
        except Exception:
            pass
        try:
            ctrl_sock.close()
        except Exception:
            pass
        try:
            os.close(lock_fd)
        except Exception:
            pass
        for path in (grant_path, sock_path, ctrl_path, lock_path):
            try:
                path.unlink()
            except FileNotFoundError:
                pass


def emit_command(args):
    raw_text = sys.stdin.read()
    raw_payload = json.loads(raw_text) if raw_text.strip() else {}
    sanitized = sanitize_hook_stdin(raw_payload, args.event)
    request = {
        "version": 1,
        "subscriptionId": "hook-emitter",
        "id": str(uuid.uuid4()),
        "token": args.token,
        "instanceId": args.instance_id,
        "generation": args.generation,
        "op": "ingest",
        "event": sanitized,
    }
    reply = send_uds_json(args.socket, request)
    if args.receipt_file:
        Path(args.receipt_file).write_text(json.dumps(reply, indent=2) + "\n", encoding="utf-8")
    hook_stdout = {"decision": "allow"} if args.event in ("PreInvocation", "PreToolUse") else {}
    sys.stdout.write(json.dumps(hook_stdout) + "\n")
    sys.stdout.flush()
    if args.strict and reply.get("status") not in ("ok", "duplicate"):
        sys.exit(1)


def catalog_command(args):
    raw_text = sys.stdin.read()
    raw_payload = json.loads(raw_text) if raw_text.strip() else {}
    raw_entries = raw_payload.get("entries", raw_payload) if isinstance(raw_payload, dict) else raw_payload
    if not isinstance(raw_entries, list):
        raise ValueError("catalog stdin payload must be a JSON array or object with 'entries'")
    sanitized_entries = [sanitize_catalog_entry(item) for item in raw_entries]
    request = {
        "version": 1,
        "subscriptionId": "catalog-seeder",
        "id": str(uuid.uuid4()),
        "token": args.token,
        "instanceId": args.instance_id,
        "generation": args.generation,
        "op": "seed-catalog",
        "schemaVersion": CATALOG_NAV_SCHEMA_VERSION,
        "entries": sanitized_entries,
    }
    if isinstance(raw_payload, dict) and isinstance(raw_payload.get("selected"), dict):
        request["selected"] = {
            "conversationId": raw_payload["selected"].get("conversationId", ""),
            "workspacePath": normalize_workspace_uri(raw_payload["selected"].get("workspacePath", "")) or "",
        }
    reply = send_uds_json(args.socket, request)
    if args.receipt_file:
        Path(args.receipt_file).write_text(json.dumps(reply, indent=2) + "\n", encoding="utf-8")
    sys.stdout.write(json.dumps(reply) + "\n")
    sys.stdout.flush()
    if args.strict and reply.get("status") != "ok":
        sys.exit(1)


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)

    serve = sub.add_parser("serve", help="Run the Antigravity UDS metadata bridge")
    serve.add_argument("--directory", required=True, help="0700 private directory")
    serve.add_argument("--instance-id", default="antigravity-local", help="Antigravity instance identifier")
    serve.add_argument("--generation", type=int, default=1, help="Bridge generation (> 0)")
    serve.add_argument("--workspace", action="append", required=True, help="Allowed workspace path (repeatable)")
    serve.add_argument("--ttl", type=float, default=120.0, help="Daemon lifetime limit in seconds")
    serve.add_argument("--heartbeat-ttl", type=float, default=None, help="Optional stream heartbeat expiry in seconds")
    serve.add_argument("--catalog-file", default=None, help="Optional pre-existing session catalog JSON file")
    serve.add_argument("--disable-catalog-schema", action="store_true", help="Fail-closed test flag for missing schema")

    emit = sub.add_parser("emit", help="Sanitize one official Antigravity hook stdin payload and forward metadata")
    emit.add_argument("--socket", required=True, help="Path to antigravity.sock")
    emit.add_argument("--token", required=True, help="Capability token")
    emit.add_argument("--instance-id", default="antigravity-local", help="Antigravity instance identifier")
    emit.add_argument("--generation", type=int, default=1, help="Bridge generation (> 0)")
    emit.add_argument("--event", required=True, choices=VALID_EVENTS, help="Official hook event name")
    emit.add_argument("--receipt-file", default=None, help="Optional file to record daemon reply")
    emit.add_argument("--strict", action="store_true", help="Exit non-zero if bridge rejects the event")

    catalog = sub.add_parser("catalog", help="Sanitize and seed existing Antigravity conversation catalog entries")
    catalog.add_argument("--socket", required=True, help="Path to antigravity.sock")
    catalog.add_argument("--token", required=True, help="Capability token")
    catalog.add_argument("--instance-id", default="antigravity-local", help="Antigravity instance identifier")
    catalog.add_argument("--generation", type=int, default=1, help="Bridge generation (> 0)")
    catalog.add_argument("--receipt-file", default=None, help="Optional file to record daemon reply")
    catalog.add_argument("--strict", action="store_true", help="Exit non-zero if bridge rejects the catalog")

    args = parser.parse_args()
    if args.cmd == "serve":
        serve_command(args)
    elif args.cmd == "emit":
        emit_command(args)
    else:
        catalog_command(args)


if __name__ == "__main__":
    main()
