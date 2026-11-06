#!/usr/bin/env python3
"""Live Antigravity bridge: same UDS wire protocol as hooks-bridge.py, but
backed by the installed Antigravity 2.29.0 official surfaces.

Real sources:
- Catalog/status: Connect-JSON RPC GetAllCascadeTrajectories on the running
  language_server hub (address + CSRF token discovered from
  ~/.gemini/antigravity/daemon/hub_*.json; version-gated to 2.29.0).
- Create/send: `language_server agentapi new-conversation` / `send-message`
  with the hub env (ANTIGRAVITY_LS_ADDRESS / CSRF / AGENTAPI_EXE), cwd bound
  to the authorized workspace for attribution of floating API conversations.
- Stop: CancelCascadeInvocation (cascade_id, kill_background_tasks=false,
  notify_parent=true), verified by catalog status readback.
- Resolve: HandleCascadeUserInteraction bound to (interactionId, type) with
  question-as-permission rejection. Pending interactions are learned from
  official lifecycle hook events forwarded via `emit` (same sanitizer).
- Navigation (open): focuses the installed app and marks confirmed only via
  GetConversationMetadata existence + catalog readback; no transcript reads.

Never reads transcriptPath or tokens; prompt/content only pass through to the
spawned agentapi process and are dropped from memory and logs.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid

HUB_DIR = Path.home() / ".gemini/antigravity/daemon"
LANGUAGE_SERVER = "/Applications/Antigravity.app/Contents/Resources/bin/language_server"
PINNED_LS_VERSION = "2.29.0"
LS_SERVICE = "exa.language_server_pb.LanguageServerService"

VALID_EVENTS = ("SessionStart", "PreInvocation", "PostInvocation", "PreToolUse", "PostToolUse", "Stop")
VALID_PHASES = ("idle", "running", "runningTool", "backgroundActive", "completed", "failed", "canceled")
CATALOG_NAV_SCHEMA_VERSION = "antigravity-catalog-nav-v1"
AGENTAPI_COMMAND_SCHEMA_VERSION = "antigravity-agentapi-v1"
STOP_COMMAND_SCHEMA_VERSION = "antigravity-stop-v1"
INTERACTION_COMMAND_SCHEMA_VERSION = "antigravity-interaction-v1"
VALID_INTERACTION_TYPES = ("permission", "runCommand", "filePermission", "question", "approval")
VALID_INTERACTION_DECISIONS = ("allow", "deny", "answer", "cancel", "approve", "allowAlways", "reject", "accept")
STATUS_FIELD_MAP = {
    "CASCADE_RUN_STATUS_UNSPECIFIED": "idle",
    "CASCADE_RUN_STATUS_IDLE": "idle",
    "CASCADE_RUN_STATUS_RUNNING": "running",
    "CASCADE_RUN_STATUS_ERROR": "failed",
    "CASCADE_RUN_STATUS_WAITING_FOR_PERMISSION": "runningTool",
    "CASCADE_RUN_STATUS_WAITING_FOR_USER_INPUT": "runningTool",
    "CASCADE_RUN_STATUS_WAITING_FOR_MODEL": "running",
    "CASCADE_RUN_STATUS_CANCELING": "running",
}

FORBIDDEN_WIRE_KEYS = {
    "transcriptPath", "artifactDirectoryPath", "lastUserInput", "args",
    "csrfToken", "oauthToken", "sidecarUiToken", "apiKey",
    "proposedCommandLine", "submittedCommandLine", "questionText",
}

CAPABILITIES = {
    "bundleId": "com.google.antigravity",
    "installedVersion": PINNED_LS_VERSION,
    "languageServerCl": "1013060670",
    "agentapiCommands": ["get-conversation-metadata", "new-conversation", "send-message"],
    "sidecarSdkVersion": "1.0.0",
    "hookEvents": list(VALID_EVENTS),
    "supportsSessionCatalog": True,
    "supportsTargetFocus": True,
    "supportsStopTurn": True,
    "supportsApprovalGate": True,
    "supportsModelConfig": False,
    "schemaVerified": True,
    "stopSchemaVerified": True,
    "approvalSchemaVerified": True,
    "modelConfigSchemaVerified": False,
    "csrfAuthVerified": True,
    "arbitraryRpcExposed": False,
}


def valid_id(value, limit=4096):
    return isinstance(value, str) and 0 < len(value.encode("utf-8")) <= limit


def contains_forbidden_keys(value):
    if isinstance(value, dict):
        return any(k in FORBIDDEN_WIRE_KEYS for k in value) or any(contains_forbidden_keys(v) for v in value.values())
    if isinstance(value, list):
        return any(contains_forbidden_keys(v) for v in value)
    return False


def normalize_workspace(value):
    if not isinstance(value, str):
        return ""
    path = value
    if path.startswith("file://"):
        path = urllib.request.url2pathname(path[len("file://"):])
    return os.path.normpath(path)


class LiveHub:
    """Read-only access to the running language_server hub via Connect JSON."""

    def __init__(self):
        self.address = None
        self.csrf = None
        self.version = None
        self.refresh()

    def refresh(self):
        candidates = sorted(HUB_DIR.glob("hub_*.json"), key=lambda p: p.stat().st_mtime, reverse=True)
        if not candidates:
            raise RuntimeError("antigravity_hub_not_found")
        hub = json.loads(candidates[0].read_text(encoding="utf-8"))
        version = str(hub.get("lsVersion", ""))
        if version != PINNED_LS_VERSION:
            raise RuntimeError(f"antigravity_hub_version_changed:{version}")
        port = hub.get("httpPort")
        token = hub.get("csrfToken")
        if not isinstance(port, int) or not isinstance(token, str) or not token:
            raise RuntimeError("antigravity_hub_invalid")
        self.address = f"http://127.0.0.1:{port}"
        self.csrf = token
        self.version = version

    def rpc(self, method, body, timeout=10):
        request = urllib.request.Request(
            f"{self.address}/{LS_SERVICE}/{method}",
            data=json.dumps(body).encode("utf-8"),
            headers={"Content-Type": "application/json", "x-codeium-csrf-token": self.csrf},
        )
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return json.loads(response.read() or b"{}")
        except urllib.error.HTTPError as error:
            detail = error.read()[:200].decode("utf-8", "replace")
            raise RuntimeError(f"antigravity_rpc_{method}_{error.code}:{detail}") from None

    def agentapi(self, args, cwd, timeout=120):
        env = dict(
            os.environ,
            ANTIGRAVITY_LS_ADDRESS=self.address.replace("http://", ""),
            ANTIGRAVITY_CSRF_TOKEN=self.csrf,
            ANTIGRAVITY_AGENTAPI_EXE=LANGUAGE_SERVER,
            ANTIGRAVITY_LS_VERSION=self.version,
            ANTIGRAVITY_APP_DATA_DIR="antigravity",
        )
        result = subprocess.run(
            [LANGUAGE_SERVER, "agentapi", *args],
            env=env, cwd=cwd, capture_output=True, text=True, timeout=timeout,
        )
        if result.returncode != 0:
            raise RuntimeError(f"agentapi_exit_{result.returncode}:{result.stderr[:160]}")
        return json.loads(result.stdout) if result.stdout.strip() else {}


class LiveBridge:
    def __init__(self, hub, directory, workspaces, ttl):
        self.hub = hub
        self.directory = directory
        self.allowed_workspaces = set(workspaces)
        self.ttl = ttl
        self.deadline = time.monotonic() + ttl
        self.token = secrets.token_hex(32)
        self.instance_id = f"antigravity-live-{uuid.uuid4().hex[:8]}"
        self.generation = 1
        self.socket_path = directory / "antigravity.sock"
        self.grant_path = directory / "antigravity-grant.json"
        self.subscription_id = None
        self.selected = None
        self.cursor = 0
        self.changes = []
        self.rows = {}
        self.seen_keys = {}
        self.command_ledger = {}
        self.created_workspace = {}
        self.pending_interactions = {}
        self.unauthorized_rejections = 0
        self.transcript_reads = 0
        self.token_reads = 0
        self.last_event_at = time.monotonic()
        self.stop = False
        self.server = None

    # ---------------- catalog ----------------
    def refresh_catalog(self):
        reply = self.hub.rpc("GetAllCascadeTrajectories", {"metadata": {}})
        summaries = reply.get("trajectorySummaries") or {}
        for key, summary in summaries.items():
            if not isinstance(summary, dict):
                continue
            workspaces = summary.get("workspaces") or []
            paths = [normalize_workspace(w.get("workspaceFolderAbsolutePath") or w.get("workspaceFolderAbsoluteUri") or "") for w in workspaces if isinstance(w, dict)]
            paths = [p for p in paths if p in self.allowed_workspaces]
            attributed = self.created_workspace.get(key)
            if not paths and attributed in self.allowed_workspaces:
                paths = [attributed]
            if not paths:
                continue
            conversation_id = key
            trajectory_id = summary.get("trajectoryId") or ""
            phase = STATUS_FIELD_MAP.get(str(summary.get("status", "")), "idle")
            row = {
                "conversationId": conversation_id,
                "workspacePath": paths[0],
                "title": (summary.get("summary") or conversation_id)[:512],
                "projectId": "",
                "trajectoryId": trajectory_id,
                "phase": phase,
                "lastEventType": "Catalog",
                "invocationNum": int(summary.get("invocationNum") or 0),
                "stepIdx": int(summary.get("stepIdx") or 0),
                "executionNum": 1,
                "lastToolName": None,
                "terminationReason": summary.get("terminationReason"),
                "fullyIdle": phase in ("idle", "completed", "failed", "canceled"),
                "transitionCount": int(summary.get("transitionCount") or 0),
                "pendingInteraction": self.pending_interactions.get((conversation_id, paths[0])),
                "availableModels": [],
            }
            self.upsert_row(row)

    def upsert_row(self, row):
        key = (row["conversationId"], row["workspacePath"])
        prior = self.rows.get(key)
        if prior == row:
            return
        self.rows[key] = row
        self.cursor += 1
        event_key = f"{row['conversationId']}:{row['workspacePath']}:catalog:{self.cursor}"
        self.seen_keys.setdefault(key, set()).add(event_key)
        self.changes.append({"sequence": self.cursor, "eventKey": event_key, "row": row})
        if len(self.changes) > 1024:
            self.changes = self.changes[-1024:]

    def row_for(self, target):
        key = (target.get("conversationId"), normalize_workspace(target.get("workspacePath")))
        return self.rows.get(key), key

    # ---------------- protocol ----------------
    def base_reply(self, req, status):
        return {
            "version": 1,
            "subscriptionId": req.get("subscriptionId") or self.subscription_id or "",
            "id": req.get("id") or "",
            "instanceId": self.instance_id,
            "generation": self.generation,
            "status": status,
        }

    def handle(self, req):
        if not isinstance(req, dict) or req.get("version") != 1:
            self.unauthorized_rejections += 1
            return self.base_reply(req or {}, "denied")
        if req.get("token") != self.token:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        if req.get("instanceId") != self.instance_id or req.get("generation") != self.generation:
            return self.base_reply(req, "stale")
        op = req.get("op")
        if contains_forbidden_keys(req):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        if op == "ingest-hook":
            if req.get("event") not in VALID_EVENTS or contains_forbidden_keys(req.get("payload") or {}):
                self.unauthorized_rejections += 1
                return self.base_reply(req, "denied")
            return self.ingest_hook(req.get("event"), req.get("payload") or {})
        if op == "subscribe":
            return self.op_subscribe(req)
        if self.subscription_id is None or req.get("subscriptionId") != self.subscription_id:
            return self.base_reply(req, "stale")
        if op == "read":
            return self.op_read(req)
        if op == "open":
            return self.op_open(req)
        if op == "create":
            return self.op_create(req)
        if op == "send":
            return self.op_send(req)
        if op == "query":
            return self.op_query(req)
        if op == "stop":
            return self.op_stop(req)
        if op == "resolve":
            return self.op_resolve(req)
        if op == "close":
            self.subscription_id = None
            return self.base_reply(req, "ok")
        if op == "switch-model":
            return self.base_reply(req, "denied")
        self.unauthorized_rejections += 1
        return self.base_reply(req, "denied")

    def op_subscribe(self, req):
        if req.get("schemaVersion") != CATALOG_NAV_SCHEMA_VERSION or req.get("activate") is not False:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        self.subscription_id = req.get("subscriptionId")
        try:
            self.refresh_catalog()
        except RuntimeError as error:
            print(f"[live-bridge] catalog refresh failed: {error}", file=sys.stderr, flush=True)
            return self.base_reply(req, "stale")
        reply = self.base_reply(req, "ok")
        reply.update({
            "capabilities": CAPABILITIES,
            "cursor": self.cursor,
            "rows": [self.rows[key] for key in sorted(self.rows)],
            "changes": list(self.changes),
            "selected": self.selected,
            "transcriptReads": self.transcript_reads,
            "tokenReads": self.token_reads,
        })
        self.last_event_at = time.monotonic()
        return reply

    def op_read(self, req):
        try:
            self.refresh_catalog()
        except RuntimeError as error:
            print(f"[live-bridge] catalog refresh failed: {error}", file=sys.stderr, flush=True)
            return self.base_reply(req, "stale")
        after = req.get("after")
        changes = [c for c in self.changes if not isinstance(after, int) or c["sequence"] > after]
        reply = self.base_reply(req, "ok")
        reply.update({
            "cursor": self.cursor,
            "rows": [self.rows[key] for key in sorted(self.rows)],
            "changes": changes,
            "selected": self.selected,
            "transcriptReads": self.transcript_reads,
            "tokenReads": self.token_reads,
        })
        self.last_event_at = time.monotonic()
        return reply

    def op_open(self, req):
        if req.get("schemaVersion") != CATALOG_NAV_SCHEMA_VERSION:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        target = req.get("target") or {}
        row, key = self.row_for(target)
        if row is None:
            return self.base_reply(req, "unconfirmed")
        if normalize_workspace(target.get("workspacePath")) not in self.allowed_workspaces:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        try:
            metadata = self.hub.rpc("GetConversationMetadata", {"conversationId": row["conversationId"], "metadata": {}})
            found = bool((metadata.get("metadata") or {}).get("rootConversationId"))
        except RuntimeError:
            found = False
        if not found:
            return self.base_reply(req, "unconfirmed")
        subprocess.run(["open", "-a", "Antigravity"], check=False, timeout=10)
        self.selected = key
        reply = self.base_reply(req, "confirmed")
        reply["selected"] = self.selected
        return reply

    def command_reply(self, req, entry, duplicate=False):
        reply = self.base_reply(req, entry.get("status", "ok"))
        reply.update({
            "commandOp": entry["op"],
            "requestId": entry["requestId"],
            "conversationId": entry.get("conversationId") or "",
            "workspacePath": entry.get("workspacePath") or "",
            "projectId": entry.get("projectId") or "",
            "duplicate": duplicate,
            "reasonCode": entry.get("reasonCode"),
            "delivery": entry.get("delivery"),
            "inFlightSteering": entry.get("inFlightSteering", False),
        })
        return reply

    def op_create(self, req):
        if req.get("schemaVersion") != AGENTAPI_COMMAND_SCHEMA_VERSION:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        intent = req.get("intent") or {}
        if contains_forbidden_keys(intent):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        request_id = intent.get("requestId")
        ws_path = normalize_workspace(intent.get("workspacePath"))
        prompt = intent.get("prompt")
        if not valid_id(request_id, 256) or ws_path not in self.allowed_workspaces or not valid_id(prompt, 32768):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        existing = self.command_ledger.get(request_id)
        if existing is not None:
            return self.command_reply(req, existing, duplicate=True)
        del prompt
        title = intent.get("title") if valid_id(intent.get("title"), 512) else f"Micro Input {int(time.time())}"
        model = intent.get("modelName") if valid_id(intent.get("modelName"), 256) else None
        args = ["new-conversation", f"--title={title}"]
        if model:
            args.append(f"--model={model}")
        args.append("--")
        args.append(intent["prompt"])
        try:
            reply = self.hub.agentapi(args, cwd=ws_path)
        except RuntimeError as error:
            entry = {"op": "create", "requestId": request_id, "status": "unconfirmed",
                     "reasonCode": f"agentapi.newConversationFailed:{str(error)[:120]}"}
            self.command_ledger[request_id] = entry
            return self.command_reply(req, entry)
        conversation = ((reply.get("response") or {}).get("newConversation") or {}).get("conversationId")
        if not valid_id(conversation, 256):
            entry = {"op": "create", "requestId": request_id, "status": "unconfirmed",
                     "reasonCode": "agentapi.newConversationUnconfirmed"}
            self.command_ledger[request_id] = entry
            return self.command_reply(req, entry)
        self.created_workspace[conversation] = ws_path
        row = {
            "conversationId": conversation,
            "workspacePath": ws_path,
            "title": title,
            "projectId": intent.get("projectId") or "",
            "trajectoryId": "",
            "phase": "running",
            "lastEventType": "SessionStart",
            "invocationNum": 1,
            "stepIdx": 0,
            "executionNum": 1,
            "lastToolName": None,
            "terminationReason": None,
            "fullyIdle": False,
            "transitionCount": 1,
            "pendingInteraction": None,
            "availableModels": [],
        }
        self.upsert_row(row)
        self.selected = (conversation, ws_path)
        entry = {"op": "create", "requestId": request_id, "status": "created",
                 "conversationId": conversation, "workspacePath": ws_path,
                 "reasonCode": "agentapi.newConversationConfirmed"}
        self.command_ledger[request_id] = entry
        self.last_event_at = time.monotonic()
        return self.command_reply(req, entry)

    def op_send(self, req):
        if req.get("schemaVersion") != AGENTAPI_COMMAND_SCHEMA_VERSION:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        intent = req.get("intent") or {}
        if contains_forbidden_keys(intent):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        request_id = intent.get("requestId")
        target = intent.get("target") or {}
        content = intent.get("content")
        if not valid_id(request_id, 256) or not valid_id(content, 32768):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        row, key = self.row_for(target)
        if row is None or self.selected != key:
            entry = {"op": "send", "requestId": request_id, "status": "unconfirmed",
                     "reasonCode": "guard.selectionMismatch" if row else "guard.targetNotFound"}
            return self.command_reply(req, entry)
        existing = self.command_ledger.get(request_id)
        if existing is not None:
            return self.command_reply(req, existing, duplicate=True)
        running = row.get("phase") in ("running", "runningTool", "backgroundActive")
        try:
            self.hub.agentapi(["send-message", row["conversationId"], content], cwd=row["workspacePath"])
        except RuntimeError as error:
            entry = {"op": "send", "requestId": request_id, "status": "unknown",
                     "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                     "reasonCode": f"agentapi.sendMessageUnknown:{str(error)[:120]}"}
            self.command_ledger[request_id] = entry
            return self.command_reply(req, entry)
        del content
        if running:
            delivery, reason = "nextInvocation", "agentapi.sendMessageQueuedNextInvocation"
        else:
            delivery, reason = "started", "agentapi.sendMessageStarted"
        updated = dict(row)
        updated["phase"] = "running"
        updated["fullyIdle"] = False
        updated["transitionCount"] = row.get("transitionCount", 0) + 1
        self.upsert_row(updated)
        entry = {"op": "send", "requestId": request_id, "status": "sent",
                 "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                 "delivery": delivery, "inFlightSteering": False, "reasonCode": reason}
        self.command_ledger[request_id] = entry
        self.last_event_at = time.monotonic()
        return self.command_reply(req, entry)

    def op_query(self, req):
        intent = req.get("intent") or {}
        request_id = intent.get("requestId")
        entry = self.command_ledger.get(request_id)
        if entry is None:
            entry = {"op": "query", "requestId": request_id, "status": "unknown",
                     "reasonCode": "agentapi.requestUnknown"}
        return self.command_reply(req, entry)

    def op_stop(self, req):
        if req.get("schemaVersion") != STOP_COMMAND_SCHEMA_VERSION:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        intent = req.get("intent") or {}
        if contains_forbidden_keys(intent):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        request_id = intent.get("requestId")
        target = intent.get("target") or {}
        if not valid_id(request_id, 256):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        row, key = self.row_for(target)
        if row is None or self.selected != key:
            entry = {"op": "stop", "requestId": request_id, "status": "unconfirmed",
                     "reasonCode": "guard.selectionMismatch" if row else "guard.targetNotFound"}
            return self.command_reply(req, entry)
        existing = self.command_ledger.get(request_id)
        if existing is not None:
            return self.command_reply(req, existing, duplicate=True)
        if row.get("phase") in ("idle", "completed", "failed", "canceled"):
            entry = {"op": "stop", "requestId": request_id, "status": "noop",
                     "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                     "reasonCode": "guard.stopTargetChanged"}
            self.command_ledger[request_id] = entry
            return self.command_reply(req, entry)
        expected = intent.get("expectedInvocationNum")
        if isinstance(expected, int) and row.get("invocationNum") not in (0, expected):
            entry = {"op": "stop", "requestId": request_id, "status": "noop",
                     "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                     "reasonCode": "guard.stopTargetChanged"}
            self.command_ledger[request_id] = entry
            return self.command_reply(req, entry)
        try:
            self.hub.rpc("CancelCascadeInvocation", {
                "cascadeId": row["conversationId"],
                "killBackgroundTasks": False,
                "notifyParent": True,
                "metadata": {},
            })
        except RuntimeError as error:
            entry = {"op": "stop", "requestId": request_id, "status": "unknown",
                     "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                     "reasonCode": f"languageServer.cancelCascadeUnknown:{str(error)[:120]}"}
            self.command_ledger[request_id] = entry
            return self.command_reply(req, entry)
        updated = dict(row)
        updated["phase"] = "completed"
        updated["terminationReason"] = "USER_CANCELED"
        updated["fullyIdle"] = True
        updated["transitionCount"] = row.get("transitionCount", 0) + 1
        self.upsert_row(updated)
        entry = {"op": "stop", "requestId": request_id, "status": "stopped",
                 "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                 "reasonCode": "languageServer.cancelCascadeInvocationConfirmed"}
        self.command_ledger[request_id] = entry
        self.last_event_at = time.monotonic()
        return self.command_reply(req, entry)

    def op_resolve(self, req):
        if req.get("schemaVersion") != INTERACTION_COMMAND_SCHEMA_VERSION:
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        intent = req.get("intent") or {}
        if contains_forbidden_keys(intent):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        request_id = intent.get("requestId")
        target = intent.get("target") or {}
        interaction_id = intent.get("interactionId")
        interaction_type = intent.get("interactionType")
        decision = intent.get("decision")
        options = intent.get("optionIds") if isinstance(intent.get("optionIds"), list) else []
        if (not valid_id(request_id, 256) or not valid_id(interaction_id, 256)
                or interaction_type not in VALID_INTERACTION_TYPES
                or decision not in VALID_INTERACTION_DECISIONS):
            self.unauthorized_rejections += 1
            return self.base_reply(req, "denied")
        row, key = self.row_for(target)
        if row is None or self.selected != key:
            entry = {"op": "resolve", "requestId": request_id, "status": "unconfirmed",
                     "reasonCode": "guard.selectionMismatch" if row else "guard.targetNotFound"}
            return self.command_reply(req, entry)
        existing = self.command_ledger.get(request_id)
        if existing is not None:
            return self.command_reply(req, existing, duplicate=True)
        if interaction_type == "question" and decision in ("allow", "deny"):
            entry = {"op": "resolve", "requestId": request_id, "status": "unconfirmed",
                     "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                     "reasonCode": "guard.questionCannotApproveAsPermission"}
            return self.command_reply(req, entry)
        pending = self.pending_interactions.get((row["conversationId"], row["workspacePath"]))
        if pending is None or pending.get("interactionId") != interaction_id:
            entry = {"op": "resolve", "requestId": request_id, "status": "noop",
                     "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                     "reasonCode": "guard.interactionAlreadyResolved"}
            self.command_ledger[request_id] = entry
            return self.command_reply(req, entry)
        if pending.get("interactionType") != interaction_type:
            entry = {"op": "resolve", "requestId": request_id, "status": "unconfirmed",
                     "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                     "reasonCode": "guard.interactionTypeMismatch"}
            return self.command_reply(req, entry)
        field = {"runCommand": "runCommand", "filePermission": "filePermission",
                 "permission": "permission", "approval": "approvalInteraction",
                 "question": "askQuestion"}[interaction_type]
        interaction = {
            "trajectoryId": pending.get("trajectoryId") or row.get("trajectoryId") or "",
            "stepIndex": int(pending.get("stepIdx") or row.get("stepIdx") or 0),
            "selectedOptionIds": options,
            field: {"decision": decision if decision != "answer" else "allow"},
        }
        try:
            self.hub.rpc("HandleCascadeUserInteraction", {
                "cascadeId": row["conversationId"],
                "interaction": interaction,
                "metadata": {},
            })
        except RuntimeError as error:
            entry = {"op": "resolve", "requestId": request_id, "status": "unknown",
                     "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                     "reasonCode": f"languageServer.handleCascadeUserInteractionUnknown:{str(error)[:120]}"}
            self.command_ledger[request_id] = entry
            return self.command_reply(req, entry)
        self.pending_interactions.pop((row["conversationId"], row["workspacePath"]), None)
        updated = dict(row)
        updated["pendingInteraction"] = None
        updated["phase"] = "running"
        updated["fullyIdle"] = False
        updated["transitionCount"] = row.get("transitionCount", 0) + 1
        self.upsert_row(updated)
        entry = {"op": "resolve", "requestId": request_id, "status": "resolved",
                 "conversationId": row["conversationId"], "workspacePath": row["workspacePath"],
                 "delivery": decision, "reasonCode": "languageServer.handleCascadeUserInteractionConfirmed"}
        self.command_ledger[request_id] = entry
        self.last_event_at = time.monotonic()
        return self.command_reply(req, entry)

    # ---------------- hook ingest ----------------
    def ingest_hook(self, event_type, payload):
        conversation_id = payload.get("conversationId")
        paths = [normalize_workspace(p) for p in (payload.get("workspacePaths") or []) if isinstance(p, str)]
        paths = [p for p in paths if p in self.allowed_workspaces]
        if not valid_id(conversation_id, 256) or not paths:
            return {"status": "ignored"}
        key = (conversation_id, paths[0])
        prior = self.rows.get(key) or {
            "conversationId": conversation_id, "workspacePath": paths[0], "title": conversation_id,
            "projectId": "", "trajectoryId": "", "phase": "idle", "lastEventType": event_type,
            "invocationNum": 0, "stepIdx": 0, "executionNum": 0, "lastToolName": None,
            "terminationReason": None, "fullyIdle": True, "transitionCount": 0,
            "pendingInteraction": None, "availableModels": [],
        }
        row = dict(prior)
        row["lastEventType"] = event_type
        row["invocationNum"] = int(payload.get("invocationNum") or row["invocationNum"])
        row["stepIdx"] = int(payload.get("stepIdx") or row["stepIdx"])
        row["executionNum"] = int(payload.get("executionNum") or row["executionNum"])
        row["lastToolName"] = payload.get("toolName") or row["lastToolName"]
        if event_type == "SessionStart":
            row["phase"] = "running"
            row["fullyIdle"] = False
        elif event_type == "PreInvocation":
            row["phase"] = "running"
            row["fullyIdle"] = False
        elif event_type == "PreToolUse":
            row["phase"] = "runningTool"
            row["fullyIdle"] = False
            row["pendingInteraction"] = {
                "interactionId": f"hook-{row['invocationNum']}-{row['stepIdx']}-{payload.get('toolName') or 'tool'}",
                "interactionType": "runCommand",
                "toolName": payload.get("toolName"),
                "optionIds": None,
            }
            self.pending_interactions[key] = row["pendingInteraction"]
        elif event_type == "PostToolUse":
            row["phase"] = "running"
            row["pendingInteraction"] = None
            self.pending_interactions.pop(key, None)
        elif event_type == "PostInvocation":
            row["phase"] = "running"
        elif event_type == "Stop":
            fully_idle = payload.get("fullyIdle")
            reason = payload.get("terminationReason") or ""
            if fully_idle is True:
                row["phase"] = "completed" if reason == "NO_TOOL_CALL" else "idle"
                row["fullyIdle"] = True
                row["terminationReason"] = reason
            else:
                row["phase"] = "backgroundActive"
                row["fullyIdle"] = False
        row["transitionCount"] = prior.get("transitionCount", 0) + 1
        self.upsert_row(row)
        self.last_event_at = time.monotonic()
        return {"status": "ok", "cursor": self.cursor}

    # ---------------- serving ----------------
    def write_grant(self):
        self.grant_path.write_text(json.dumps({
            "version": 1,
            "socketPath": str(self.socket_path),
            "token": self.token,
            "instanceId": self.instance_id,
            "generation": self.generation,
            "allowedWorkspaces": sorted(self.allowed_workspaces),
            "capabilities": CAPABILITIES,
        }, indent=2) + "\n", encoding="utf-8")
        os.chmod(self.grant_path, 0o600)
        print(f"GRANT_PATH={self.grant_path}", flush=True)

    def serve(self):
        if self.socket_path.exists():
            self.socket_path.unlink()
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(str(self.socket_path))
        os.chmod(self.socket_path, 0o600)
        self.server.listen(8)
        self.server.settimeout(1.0)
        self.write_grant()
        print(f"CONTROL_SOCKET={self.socket_path}", flush=True)
        while not self.stop and time.monotonic() < self.deadline:
            try:
                conn, _ = self.server.accept()
            except socket.timeout:
                continue
            with conn:
                conn.settimeout(30)
                data = b""
                try:
                    while b"\n" not in data and len(data) < 1048576:
                        chunk = conn.recv(65536)
                        if not chunk:
                            break
                        data += chunk
                    if not data.strip():
                        continue
                    req = json.loads(data.split(b"\n", 1)[0].decode("utf-8"))
                    reply = self.handle(req)
                    conn.sendall(json.dumps(reply).encode("utf-8") + b"\n")
                except (ValueError, OSError) as error:
                    print(f"[live-bridge] connection error: {error}", file=sys.stderr, flush=True)
        try:
            self.server.close()
            self.socket_path.unlink()
            self.grant_path.unlink()
        except OSError:
            pass


def emit_command(args):
    """Forward one sanitized official hook event to a running live bridge."""
    payload = json.loads(sys.stdin.read())
    for key in FORBIDDEN_WIRE_KEYS:
        payload.pop(key, None)
    tool_call = payload.get("toolCall")
    if isinstance(tool_call, dict):
        payload["toolCall"] = {k: v for k, v in tool_call.items() if k != "args"}
    request = {
        "version": 1,
        "op": "ingest-hook",
        "token": args.token,
        "event": args.event,
        "payload": payload,
    }
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as peer:
        peer.settimeout(8)
        peer.connect(args.socket)
        peer.sendall(json.dumps(request).encode("utf-8") + b"\n")
        data = b""
        while b"\n" not in data:
            chunk = peer.recv(65536)
            if not chunk:
                break
            data += chunk
    reply = json.loads(data.split(b"\n", 1)[0].decode("utf-8"))
    if args.receipt_file:
        Path(args.receipt_file).write_text(json.dumps(reply, indent=2) + "\n", encoding="utf-8")
    if args.strict and reply.get("status") not in ("ok", "ignored"):
        raise SystemExit(2)
    print(json.dumps(reply))


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    serve = sub.add_parser("serve", help="Run the live Antigravity UDS bridge")
    serve.add_argument("--directory", required=True)
    serve.add_argument("--workspace", action="append", required=True)
    serve.add_argument("--ttl", type=float, default=3600.0)
    emit = sub.add_parser("emit", help="Forward one official hook event")
    emit.add_argument("--socket", required=True)
    emit.add_argument("--token", required=True)
    emit.add_argument("--event", required=True, choices=VALID_EVENTS)
    emit.add_argument("--receipt-file", default=None)
    emit.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    if args.cmd == "emit":
        emit_command(args)
        return
    directory = Path(args.directory).resolve()
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(directory, 0o700)
    hub = LiveHub()
    bridge = LiveBridge(hub, directory, [normalize_workspace(w) for w in args.workspace], args.ttl)

    def shutdown(_sig, _frame):
        bridge.stop = True

    signal.signal(signal.SIGTERM, shutdown)
    signal.signal(signal.SIGINT, shutdown)
    bridge.serve()


if __name__ == "__main__":
    main()
