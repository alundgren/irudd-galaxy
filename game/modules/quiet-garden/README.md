# Quiet Garden

A self-contained place with a keeper and one flower. Give the flower three cups of
water to bring it to full size, then plant another seed. Each click changes the
flower immediately; there is no waiting timer.

The folder is `quiet-garden`; the module ID is `quiet_garden`, following the
installed manifest identifier rules. Requires engine 0.5.0 or newer.

Run from the repository root:

```text
galaxy-gm check game/modules/quiet-garden
galaxy-gm test game/modules/quiet-garden
galaxy play --modules game/modules/quiet-garden
```

To replay the three-second click journey and take three screenshots:

```text
galaxy play --modules game/modules/quiet-garden --inputs game/modules/quiet-garden/tests/play.inputs --ticks 90
```

Set `GALAXY_SESSIONS` to a folder outside the checkout to choose where the session
recording, save and note screenshots go. The bot waters at ticks 15, 30 and 45,
then plants again at tick 70. The `.inputs` suffix keeps this window bot separate
from the `tests/*.toml` headless scenarios.

The tests cover growth, excess watering, the single bloom fact and planting again.
Recipes are the source for the included models; remake them with
`galaxy-gm model make game/modules/quiet-garden` after editing a recipe.
