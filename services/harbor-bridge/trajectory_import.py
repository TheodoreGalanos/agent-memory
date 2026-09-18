"""Trusted ATIF connector. It preserves recorded actions separately from speech.

Use Harbor's installed models to validate the declared ATIF format; this module
never fetches referenced media or treats a narrated action as an executed call.
"""

import json
from datetime import datetime
from uuid import UUID


def import_atif(
    document,
    *,
    source_identity,
    objective,
    imported_at,
    actor_id=None,
    synthetic=False,
    selected_steps=(),
):
    from harbor.models.trajectories.trajectory import Trajectory

    if not isinstance(source_identity, str) or not source_identity.strip():
        raise ValueError("A stable source identity is required")
    if not objective.strip():
        raise ValueError("An objective is required")
    acquired = _time(imported_at)
    if actor_id is not None:
        UUID(actor_id)
    trajectory = Trajectory.model_validate(document)
    events = []
    selected_steps = set(selected_steps)
    for step in trajectory.steps:
        when = _time(step.timestamp) if step.timestamp else acquired
        unknown_time = (
            []
            if step.timestamp
            else ["Event time was not recorded; import time bounds availability"]
        )
        copied = bool(step.is_copied_context)
        metadata = {
            "format": trajectory.schema_version,
            "source_identity": source_identity,
            "trajectory_id": trajectory.trajectory_id,
            "session_id": trajectory.session_id,
            "step_id": step.step_id,
            "copied_context": copied,
            "metrics": step.metrics.model_dump(mode="json") if step.metrics else None,
            "extra": step.extra,
        }

        def append(suffix, kind, content, origin, status, explicit=False):
            # Selection is an operator input, never extracted from a message.
            selected = step.step_id in selected_steps
            evidence = {**metadata, "kind": kind, "recorded_content": content}
            text = (
                content
                if isinstance(content, str)
                else json.dumps(content, ensure_ascii=False)
            )
            media = _media(content)
            uncertainty = unknown_time + (
                ["Copied context is not independent support"] if copied else []
            )
            if media:
                uncertainty += [
                    "Referenced media has not been inspected by this importer"
                ]
            events.append(
                {
                    "event_id": f"{source_identity}:{step.step_id}:{suffix}",
                    "observed_at": when.isoformat(),
                    "kind": kind,
                    "origin": origin,
                    "evidential_status": "simulation" if synthetic else status,
                    "content": {
                        "episode": source_identity,
                        "objective": objective,
                        "conditions": ["Imported trajectory"],
                        "boundary_explicit": True,
                        "retention": "temporary" if copied or synthetic else "optional",
                        "explicitly_selected": selected,
                        "explicit_contribution": explicit and not synthetic,
                        "actor_id": actor_id if explicit else None,
                        "content": {
                            "family": "knowledge",
                            "statement": f"Recorded {kind}: {text}",
                            "subject": None,
                            "predicate": None,
                            "uncertainty": [],
                            "examined_coverage": [],
                        },
                        "evidence": evidence,
                        "source_locators": [],
                        "corrects": [],
                        "based_on": [],
                        "uncertainty": uncertainty,
                        "coverage": {
                            "examined": [f"ATIF step {step.step_id} {suffix}"],
                            "unexamined": media,
                        },
                    },
                }
            )

        message = step.model_dump(mode="json")["message"]
        if message:
            append(
                "message",
                f"{step.source}_statement",
                message,
                "agent_generated" if step.source == "agent" else "observed",
                "attributed_statement",
                step.source == "user" and actor_id is not None,
            )
        for call in step.tool_calls or []:
            append(
                f"call:{call.tool_call_id}",
                "tool_call",
                call.model_dump(mode="json"),
                "observed",
                "observation",
            )
        if step.observation:
            for index, result in enumerate(step.observation.results):
                append(
                    f"result:{index}",
                    "tool_result",
                    result.model_dump(mode="json"),
                    "observed",
                    "observation",
                )
    return {"schema_version": "memory-tool-events/1", "events": events}


def _time(value):
    at = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if at.tzinfo is None:
        raise ValueError("Import and event timestamps need a timezone")
    return at


def _media(content):
    if isinstance(content, list):
        return [
            f"Uninspected {p['type']}: {p['source']['path']}"
            for p in content
            if isinstance(p, dict) and p.get("type") in ("image", "audio")
        ]
    if isinstance(content, dict):
        return _media(content.get("content"))
    return []
