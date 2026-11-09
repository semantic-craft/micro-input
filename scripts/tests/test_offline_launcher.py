import importlib.machinery
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

loader = importlib.machinery.SourceFileLoader("offline_verifier", str(Path(__file__).parents[1] / "verify-offline"))
spec = importlib.util.spec_from_loader(loader.name, loader)
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)


class OfflineEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.run = Path(self.tmp.name)
        self.evidence = self.run / "evidence"
        self.evidence.mkdir()
        self.launcher = dict(bundle_id="com.semanticcraft.microinput.qa", started_ms=100,
                             pid=123, app_path=str(self.run / "gone.app"), document_dir=str(self.run / "gone-docs"),
                             app_bundle_removed=True, documents_removed=True, preferences_removed=True)
        self.write_launcher()
        self.rows = [dict(phase="Verified", config_readback="PASS", physical_key="NOT_RUN", lighting_visual="NOT_RUN")] * 2
        self.write_rows()
        (self.evidence / "lighting.json").write_text(json.dumps(dict(status="PASS", runtime_readback="SYNTHETIC_PASS", physical_key="NOT_RUN", lighting_visual="NOT_RUN")))

    def write_launcher(self):
        (self.evidence / "launcher.json").write_text(json.dumps(self.launcher))

    def write_rows(self):
        (self.evidence / "device-operations.jsonl").write_text("\n".join(json.dumps(r) for r in self.rows))

    def test_complete_lifecycle_is_accepted_without_claiming_hardware(self):
        result = module.verify_evidence(self.run, "lighting-control", 99, lambda _: False)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["lighting_visual"], "NOT_RUN")

    def test_diagnostic_export_requires_only_sanitized_wrapper(self):
        (self.evidence / "diagnostics.json").write_text(json.dumps(dict(status="PASS", failure_stage="config_read", physical_key="NOT_RUN", lighting_visual="NOT_RUN")))
        result = module.verify_evidence(self.run, "diagnostic-export", 99, lambda _: False)
        self.assertEqual(result["diagnostic_export"], "PASS")
        (self.evidence / "diagnostics.json").write_text(json.dumps(dict(status="PASS", failure_stage="config_read", physical_key="PASS", lighting_visual="NOT_RUN")))
        with self.assertRaises(ValueError):
            module.verify_evidence(self.run, "diagnostic-export", 99, lambda _: False)

    def test_stale_evidence_live_process_cleanup_and_overclaim_fail(self):
        for modify in (lambda: self.launcher.update(started_ms=98),
                       lambda: self.launcher.update(documents_removed=False)):
            original = self.launcher.copy()
            modify(); self.write_launcher()
            with self.assertRaises(ValueError): module.verify_evidence(self.run, "config-write", 99, lambda _: False)
            self.launcher = original; self.write_launcher()
        with self.assertRaises(ValueError): module.verify_evidence(self.run, "config-write", 99, lambda _: True)
        self.rows[0]["lighting_visual"] = "PASS"; self.write_rows()
        with self.assertRaises(ValueError): module.verify_evidence(self.run, "config-write", 99, lambda _: False)

    def test_missing_runtime_evidence_is_not_silently_skipped(self):
        (self.evidence / "lighting.json").unlink()
        with self.assertRaises(FileNotFoundError): module.verify_evidence(self.run, "lighting-control", 99, lambda _: False)

    def test_dsh_navigation_requires_pending_journal_and_exact_readback(self):
        names = [
            "disabled_zero_reads",
            "same_name_exact_identity",
            "cold_target_unavailable",
            "pending_only_transitions_retained",
            "selected_id_readback",
            "duplicate_request_no_double_send",
            "queue_and_steer_confirmed_separately",
            "switch_session_no_misstop",
            "same_session_stale_stop_zero_cancel",
            "cancel_queue_remaining_not_all_stopped",
            "unprovable_turn_atomicity_refused",
            "create_workspace_bound_readback",
            "resolve_approval_recorded",
            "already_resolved_noop_zero_side_effect",
            "interaction_type_mismatch_blocked",
            "resolve_question_answered",
            "select_model_readback_confirmed",
            "unsupported_thought_level_locally_refused",
            "wrong_selected_unconfirmed",
            "out_of_scope_zero_reads",
            "deleted_target_no_redirect",
            "host_generation_rejects_old_selection",
            "journal_gap_clears_targets",
            "bridge_subscription_closed",
        ]
        payload = dict(
            status="PASS",
            checks=dict.fromkeys(names, True),
            body_reads=0,
            native_app="NOT_RUN",
            physical_key="NOT_RUN",
        )
        path = self.evidence / "dsh-navigation.json"
        audit = self.evidence / "audit.jsonl"
        audit_rows = [
            dict(action_type="dsh_open", outcome="confirmed", target="dsh", text_byte_len=None),
            dict(action_type="dsh_open", outcome="confirmed", target="dsh", text_byte_len=None),
            dict(action_type="dsh_create", outcome="confirmed", target="dsh", text_byte_len=None),
            dict(action_type="dsh_prompt", outcome="steered", target="dsh", text_byte_len=None),
            dict(action_type="dsh_prompt", outcome="queued", target="dsh", text_byte_len=None),
            dict(action_type="dsh_cancel", outcome="noop", target="dsh", text_byte_len=None),
            dict(action_type="dsh_cancel", outcome="cancelledQueueRemaining", target="dsh", text_byte_len=None),
            dict(action_type="dsh_cancel", outcome="denied", target="dsh", text_byte_len=None),
            dict(action_type="dsh_resolve", outcome="resolved", target="dsh", text_byte_len=None),
            dict(action_type="dsh_resolve", outcome="resolved", target="dsh", text_byte_len=None),
            dict(action_type="dsh_resolve", outcome="interactionTypeMismatch", target="dsh", text_byte_len=None),
            dict(action_type="dsh_resolve", outcome="resolved", target="dsh", text_byte_len=None),
            dict(action_type="dsh_switch_model", outcome="modelSwitched", target="dsh", text_byte_len=None),
            dict(action_type="dsh_switch_model", outcome="unsupportedThought", target="dsh", text_byte_len=None),
        ]
        audit.write_text("\n".join(json.dumps(r) for r in audit_rows))
        path.write_text(json.dumps(payload))
        self.assertEqual(module.verify_evidence(self.run, "dsh-navigation", 99, lambda _: False)["dsh_navigation"], "SYNTHETIC_PASS")
        for name in names:
            payload["checks"][name] = False
            path.write_text(json.dumps(payload))
            with self.assertRaises(ValueError): module.verify_evidence(self.run, "dsh-navigation", 99, lambda _: False)
            payload["checks"][name] = True
        for field in ["native_app", "physical_key"]:
            payload[field] = "PASS"
            path.write_text(json.dumps(payload))
            with self.assertRaises(ValueError): module.verify_evidence(self.run, "dsh-navigation", 99, lambda _: False)
            payload[field] = "NOT_RUN"
        payload["body_reads"] = 1
        path.write_text(json.dumps(payload))
        with self.assertRaises(ValueError): module.verify_evidence(self.run, "dsh-navigation", 99, lambda _: False)
        payload["body_reads"] = 0
        path.write_text(json.dumps(payload))
        audit.write_text(json.dumps(dict(action_type="dsh_open", outcome="delivered", target="dsh")))
        with self.assertRaises(ValueError): module.verify_evidence(self.run, "dsh-navigation", 99, lambda _: False)

    def test_antigravity_status_requires_all_checks_and_zero_transcript_reads(self):
        names = [
            "version_and_capabilities_recorded",
            "disabled_zero_reads",
            "existing_two_sessions_catalog_and_selected_readback_confirmed",
            "same_name_cross_workspace_no_collision",
            "wrong_selected_readback_unconfirmed",
            "two_sessions_isolated",
            "duplicate_events_deduplicated",
            "stop_fully_idle_false_not_completed",
            "stop_fully_idle_true_completed",
            "create_binds_project_id_and_confirms_selected",
            "two_sessions_send_only_modifies_target",
            "running_session_send_steering_verified_separately",
            "missing_or_deleted_target_send_zero_redirect",
            "timeout_unknown_reconciles_without_resend",
            "stop_selected_session_only_and_stale_noop",
            "resolve_interaction_and_question_guard",
            "switch_model_and_thinking_guard",
            "deleted_target_no_redirect",
            "missing_schema_or_auth_fails_closed",
            "l3_schema_or_csrf_invalidation_fails_closed",
            "stream_disconnect_marks_stale",
            "generation_change_marks_stale",
            "journal_gap_marks_unavailable",
            "zero_transcript_reads",
            "zero_token_reads",
            "bridge_subscription_closed",
        ]
        payload = dict(
            status="PASS",
            installed_version="2.29.0",
            language_server_cl="1013060670",
            transcript_reads=0,
            token_reads=0,
            checks=dict.fromkeys(names, True),
            native_app="NOT_RUN",
            physical_key="NOT_RUN",
        )
        path = self.evidence / "antigravity-status.json"
        audit = self.evidence / "audit.jsonl"
        path.write_text(json.dumps(payload))
        audit_rows = [
            *(dict(action_type="antigravity_open", outcome=outcome, target="antigravity")
              for outcome in ("confirmed", "confirmed", "confirmed", "unconfirmed", "unconfirmed", "denied")),
            dict(action_type="antigravity_create", outcome="unconfirmed", target="antigravity"),
            dict(action_type="antigravity_create", outcome="created", target="antigravity"),
            dict(action_type="antigravity_send", outcome="unconfirmed", target="antigravity"),
            dict(action_type="antigravity_send", outcome="started", target="antigravity"),
            dict(action_type="antigravity_send", outcome="queued", target="antigravity"),
            dict(action_type="antigravity_send", outcome="steered", target="antigravity"),
            dict(action_type="antigravity_send", outcome="unknown", target="antigravity"),
            dict(action_type="antigravity_query", outcome="queued", target="antigravity"),
            dict(action_type="antigravity_send", outcome="unconfirmed", target="antigravity"),
            dict(action_type="antigravity_stop", outcome="unconfirmed", target="antigravity"),
            dict(action_type="antigravity_stop", outcome="noop", target="antigravity"),
            dict(action_type="antigravity_stop", outcome="stopped", target="antigravity"),
            dict(action_type="antigravity_stop", outcome="denied", target="antigravity"),
            dict(action_type="antigravity_resolve", outcome="unconfirmed", target="antigravity"),
            dict(action_type="antigravity_resolve", outcome="resolved", target="antigravity"),
            dict(action_type="antigravity_resolve", outcome="resolved", target="antigravity"),
            dict(action_type="antigravity_resolve", outcome="resolved", target="antigravity"),
            dict(action_type="antigravity_resolve", outcome="noop", target="antigravity"),
            dict(action_type="antigravity_switch_model", outcome="unconfirmed", target="antigravity"),
            dict(action_type="antigravity_switch_model", outcome="modelSwitched", target="antigravity"),
            dict(action_type="antigravity_switch_model", outcome="unconfirmed", target="antigravity"),
            dict(action_type="antigravity_switch_model", outcome="modelSwitched", target="antigravity"),
        ]
        audit.write_text("\n".join(json.dumps(r) for r in audit_rows))
        verified = module.verify_evidence(self.run, "antigravity-status", 99, lambda _: False)
        self.assertEqual(verified["antigravity_status"], "SYNTHETIC_PASS")
        self.assertEqual(verified["antigravity_navigation"], "SYNTHETIC_PASS")
        self.assertEqual(verified["antigravity_agentapi"], "SYNTHETIC_PASS")
        self.assertEqual(verified["antigravity_l3_control"], "SYNTHETIC_PASS")
        self.assertEqual(verified["transcript_reads"], 0)
        self.assertEqual(verified["token_reads"], 0)
        for name in names:
            payload["checks"][name] = False
            path.write_text(json.dumps(payload))
            with self.assertRaises(ValueError):
                module.verify_evidence(self.run, "antigravity-status", 99, lambda _: False)
            payload["checks"][name] = True
        for field, wrong in [
            ("installed_version", "2.28.0"),
            ("language_server_cl", "0"),
            ("transcript_reads", 1),
            ("token_reads", 1),
            ("native_app", "PASS"),
            ("physical_key", "PASS"),
        ]:
            prior = payload[field]
            payload[field] = wrong
            path.write_text(json.dumps(payload))
            with self.assertRaises(ValueError):
                module.verify_evidence(self.run, "antigravity-status", 99, lambda _: False)
            payload[field] = prior
        audit.write_text(json.dumps(dict(action_type="antigravity_open", outcome="confirmed", target="antigravity")))
        with self.assertRaises(ValueError):
            module.verify_evidence(self.run, "antigravity-status", 99, lambda _: False)
