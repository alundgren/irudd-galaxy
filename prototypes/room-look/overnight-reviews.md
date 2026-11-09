# Overnight review record

## Plan

- Round 1 of 3: APPROVE, no blocking defects.
- Reviewer: independent Codex agent, gpt-6.1-sol, xhigh.
- Reviewed brief SHA-256: `36dac3ad11e28006d509211b97e667c7c9ef2d6e63d6d8ee8753a12d7608b2bb`.
- Requirements: [confirmed brief](overnight-brief.md).
- Findings: none. No changes required before proceeding.
- Scope: plan only. Asset quality and final preserved changes remain unreviewed.

## Asset and final change review

Owner acceptance is pending. Asset appearance reviews and technical delivery
checks below are agent assessments; the integrated change still needs review.

## Board asset review

Round 1 of 3: CHANGES REQUESTED on frozen checkpoint 3. Reviewer: gpt-6.1-sol,
xhigh. Editable source SHA-256: `b8507abe8895820672d2b2f79b40893916bc4b6178edf439245db75e4a65fb68`.
Scope: target appearance and honest export limitations. This is separate from
coordinator visual iteration and the eventual final integrated review.

The reviewer confirmed provisional look 3/5 and graphical quality 3/5. Four
appearance findings are accepted: overly regular bevel/edge treatment; incorrect
rail, block and locally lit backing values; a geometric logo instead of an
illustrated ship; and missing deliberate material wear. The next authoring pass
addresses these on the same asset. No bust advancement is accepted yet.

The conditional export result is accurately reported. Default-import outline
self-shadowing remains a documented requirement for future runtime support, not a
reason to implement a production renderer in this experiment. Neutral/oblique
comparisons and a turntable are retained as evidence requirements.

Round 2 of 3 reviewed frozen checkpoint 5. Source SHA-256:
`399c9936e7b8a1391c9aaee0565506995df852a497717eb8e1bd3049644aa8ad`.
Export SHA-256:
`5c4991011cd4e58dea7ddf152c2e5432065b40e8e454ab9814d10f70a6894d80`.
The coordinator verified both hashes. The author replaced general edge paint with
intentional UV marks, corrected local values, painted the ship and contact wear,
then corrected fastener accents, rim strokes and the bracket's rounded corner.
The reviewer will check the four accepted findings and affected appearance.

Round 2 result: APPROVE for checkpoint 5 appearance, provisional look 4/5 and
graphical quality 4/5. All four appearance findings are resolved sufficiently for
this bounded prop. Cleaner finish and smoother icon are optional polish. Advancement
to the bust is justified after the source-delivery check below.

Coordinator validation found a source-packaging defect after appearance review.
A fresh background open warns that the file is a library, and all three packed
paint images reload with fake-user retention disabled. The cleanup helper writes
only library data and does not preserve the reported retention flags. Packed pixels
and scripts are present, but normal opening and layer retention require correction.
The author is fixing packaging only, preserving the approved art and export.

Source-packaging correction verified by the coordinator on a fresh background
load with script failures mapped to exit 1. The file opens on BoardCorner with
Cam_Reference, 18 asset objects, one window and 11 screens. All three 2048-square
paint images are packed, protected and use relative paths. The validator found no
external images or private folder bytes. Exit 0, VALIDATION PASSED. Repaired source
SHA-256: `bee10cec728bb2efd44276a7fd7f9ac101e5725352d8eca05f2fd1e1ce7ab03c`.
The export hash is unchanged. The author reports an identical reference render
after repair; final review will cover the packaging helpers and delivery evidence.

## Bram study

Appearance round 1, checkpoint 5: CHANGES REQUESTED, provisional look 2/5,
character 2/5 and graphics 2/5. Four findings: reserved immature expression;
solid beard and repeated hair; inflated cap and incorrect garment construction;
smooth finish and weak lighting contrast.

Appearance round 2, checkpoint 7: CHANGES REQUESTED, scores unchanged. The solid
beard cup was resolved and warm lighting improved. Expression, temple hair,
garment construction and painted finish remained below target. A replacement
collar introduced a cape-like construction. The coordinator stopped the appearance
review loop, rejected a further facial diagnostic because of pinched mouth corners,
and retained checkpoint 7's face. The final delivery corrects the collar regression
and records the failed appearance target. It is not an accepted character asset.

Technical delivery review round 1: fresh-load, privacy, official source hashes,
CC0 metadata, preserved original geometry, export and reimport checks passed.
A fresh GLB was byte-identical: 23,785,004 bytes, 982,704 triangles, two textures,
17 materials. The delivered reimport sheet reproduced byte-for-byte. The complete
video decoded as 96 frames at 480 by 600, 24 fps. Source hash stayed unchanged.
The sole packaging finding was five redundant single-view PNG allowlist entries.
The coordinator removed them and verified every file remains locally and is
ignored by git. No new behavior was introduced. Preservation commit: `9d9fd05`.

The technical checks do not override the failed appearance result. Animation and
runtime performance remain untested.
