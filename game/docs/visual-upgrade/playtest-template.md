# Visual increment playtest

Copy this sheet into the increment's review record. The agent fills in setup,
measurements and comparisons. Andreas can answer in ordinary words here; the
agent records his answers without replacing them with its own judgement.

## Setup

- Increment and the one improvement to judge:
- Build / commit and how to start at the test:
- Previous accepted build, or none yet:
- Camera mode; for the office comparison, complete one sheet per mode:
- Reference image or motion clip:
- Resolution, physical output, display scale, quality and refresh rate:
- Test configuration label, without private machine details:
- Three things to notice:
- Focused asset inspection or five-to-ten-minute scene route:
- Target categories that must reach 4 before building on this proof:

## First impression, before hints

For isolated assets, first record what matches or misses the goal crop at the
intended display size. The room-arrival questions below are N/A until a room exists.
The agent records scene answers before explaining the intended target. Then use
the normal task instructions. Mark a repeat visit as familiar rather than a new
first impression.

- What draws your eye?
- What kind of place does this seem to be?
- Where would you click first, and why?
- Any hesitation, wrong selection or hint needed to start the ordinary task?
- Comparison order and whether the layout was already familiar:

## Andreas's scores

0 missing/unusable; 1 wrong; 2 partial; 3 acceptable for this increment;
4 matches the target in play; 5 worth keeping as a reference.
Use N/A for absent categories and untested for things not checked.

| Category | Previous | This build | One observation or moment |
| --- | --- | --- | --- |
| Look adherence | | | |
| Character adherence | | | |
| Liveness | | | |
| Graphical quality | | | |
| Motion quality | | | |
| Performance | | | |
| Readability and navigation | | | |
| Interaction clarity | | | |

- What should we keep?
- What looked or felt wrong? A screenshot, clip timestamp or plain description:
- Ready for the next increment, or revise this one?

## Agent's evidence

Keep this separate from Andreas's scores. Link actual evidence and distinguish
observations, measurements and remaining assumptions.

- Validation and tested journey:
- Screenshot / normal-speed clip / comparison:
- Twenty-second steady scene, warm-up and measurement conditions:
- Median / p95 / p99 frame interval in ms, before and after:
- Count of frames above 33.3 ms, before and after:
- Sim / scene / draw time where available; actual GPU time if supported:
- Room entry and reload duration, measured separately:
- Office comparison: camera obstruction, player-body obstruction, target picking
  and state continuity when switching views:
- Untested conditions and why:
- Asset proof: editable source, usage rights, supported runtime data, unsupported
  features and fixed comparison conditions:
- Demonstrated defects to fix before the next test:
- Requested asset correction and the edits/export/import steps needed to see
  it working again; any unexpected engine change:
- Agent's proposed next change, explained plainly:

Do not calculate an overall average. Apply the acceptance criteria in the
[plan](../visual-upgrade-plan.md). Keep all unanswered questions together when
asking Andreas for the decision.
