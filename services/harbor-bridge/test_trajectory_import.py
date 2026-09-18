import importlib.util
import unittest

from trajectory_import import import_atif


@unittest.skipUnless(
    importlib.util.find_spec("harbor"), "installed Harbor models required"
)
class TrajectoryImportTests(unittest.TestCase):
    def document(self):
        return {
            "schema_version": "ATIF-v1.8",
            "trajectory_id": "trial",
            "agent": {"name": "test", "version": "1"},
            "steps": [
                {"step_id": 1, "source": "user", "message": "I prefer brief answers."},
                {
                    "step_id": 2,
                    "source": "agent",
                    "timestamp": "2026-09-17T00:01:00Z",
                    "message": "I inspected the image.",
                    "tool_calls": [
                        {
                            "tool_call_id": "read",
                            "function_name": "read",
                            "arguments": {},
                        }
                    ],
                    "observation": {
                        "results": [
                            {
                                "source_call_id": "read",
                                "content": [
                                    {
                                        "type": "image",
                                        "source": {
                                            "media_type": "image/png",
                                            "path": "native.png",
                                        },
                                    }
                                ],
                            }
                        ]
                    },
                },
            ],
        }

    def run_import(self, doc=None, **kwargs):
        return import_atif(
            doc or self.document(),
            source_identity="trial",
            objective="Review",
            imported_at="2026-09-17T00:02:00Z",
            **kwargs
        )["events"]

    def test_observed_calls_do_not_promote_narrated_actions(self):
        events = self.run_import(actor_id="00000000-0000-0000-0000-000000000001")
        self.assertEqual(
            [e["evidential_status"] for e in events],
            [
                "attributed_statement",
                "attributed_statement",
                "observation",
                "observation",
            ],
        )
        self.assertTrue(events[0]["content"]["explicit_contribution"])
        self.assertIsNone(events[1]["content"]["evidence"]["metrics"])
        self.assertEqual(events[0]["observed_at"], "2026-09-17T00:02:00+00:00")
        self.assertIn(
            "Uninspected image: native.png",
            events[-1]["content"]["coverage"]["unexamined"],
        )
        self.assertEqual(
            events[-1]["content"]["evidence"]["recorded_content"]["source_call_id"],
            "read",
        )

    def test_simulation_selection_and_copied_context(self):
        doc = self.document()
        doc["steps"][1]["is_copied_context"] = True
        events = self.run_import(doc, synthetic=True, selected_steps=[1])
        self.assertTrue(all(e["evidential_status"] == "simulation" for e in events))
        self.assertTrue(events[0]["content"]["explicitly_selected"])
        self.assertFalse(events[-1]["content"]["explicitly_selected"])
        self.assertIn(
            "Copied context is not independent support",
            events[-1]["content"]["uncertainty"],
        )

    def test_unknown_format_and_naive_time_fail(self):
        doc = self.document()
        doc["schema_version"] = "ATIF-v99"
        with self.assertRaises(ValueError):
            self.run_import(doc)
        doc = self.document()
        doc["steps"][0]["timestamp"] = "2026-09-17T00:00:00"
        with self.assertRaises(ValueError):
            self.run_import(doc)
