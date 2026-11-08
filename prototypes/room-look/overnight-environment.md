# Overnight visual proof environment

Run deadline: 2026-11-09 04:00 UTC, 05:00 Europe/Stockholm.
The confirmed requirements are in [overnight-brief.md](overnight-brief.md).

## Verified setup

- Blender 5.3.0 Release Candidate is installed. MCP for Blender 2.2.7 connects
  successfully to addon protocol 15. Scene inspection and viewport capture work.
- The owner explicitly permits desktop use and Blender control for this task.
- The first Blender scene contained only the default cube, camera and light.
  The board implementer owns Blender until it hands the scene back.
- The MCP telemetry prompt has a persisted answer. Server configuration disables
  telemetry. Scene inspection and Blender Python calls have succeeded after the
  prompt. New integrations and paid generators remain outside scope.
- The coordinator has a working Python MCP client route if native Blender tools
  are unavailable. The implementation session exposes native Blender MCP tools.
- A timed sleep-prevention process runs until the deadline. A one-time handoff
  trigger is scheduled for 03:40 UTC, leaving twenty minutes to save and report.

## Ownership and preservation

- Current branch: `prototype/room-look`.
- Board implementation: `blender-board-lab/` only.
- Coordinator: overnight records, review decisions, scoped integration and handoff.
- Earlier office and scripted-board experiments are preserved but are not accepted
  visual references. Do not include their entire source or raw evidence by default
  when preserving this overnight increment.
- Only one agent sends Blender mutations at a time. Reviewers inspect saved
  artifacts without taking over the interactive scene.

## Evidence still required

- Reference-matched board renders and independent visual assessment.
- Save and reload of the editable Blender source.
- Standard glTF export and isolated reimport, with conversion limitations recorded.
- Asset dimensions, texture memory and dependencies appropriate to the intended use.
- Bust and limited expression evidence if the earlier quality gate allows progress.
- Local validation and independent review of the final preserved changes.
- Remote branch, draft PR and published morning handoff.

No game-runtime appearance or real-time performance is established by a Blender
render. Source-tool, exported-asset and game-runtime evidence must be labelled.
