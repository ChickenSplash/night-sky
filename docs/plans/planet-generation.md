# Planet generation: one model and a varied population

## Status and goal

**G1 and G2 complete, 14 November 2026. G3 steps 1 to 3, 4.1 and 4.2
complete, 16 November 2026; steps 4.3 to 4.5, 5 and 6 are open. G4 is proposed, not implemented.**
Current behaviour
is described in [planetary systems](../simulation/planetary-systems.md) and the
[composition guide](../simulation/planet-composition.md).

Two goals, in this order:

1. **One source of truth.** A planet's mass, radius, surface pressure, surface
   temperature, surface type and escaped material are decided once by one
   physical model. The layered solve may add detail to that answer; it may not
   change it.
   - **A visit shows the final planet** (goal extended 16 November 2026).
     Nothing drawn or simulated changes after generation: not shape,
     atmosphere, surface or mass field. Anything that arrives later is
     interior detail for the inspector only, so the client's swap of one
     result for another, and every rule guarding it, can be deleted.
2. **A realistic and highly varied population.** Formation priors follow
   physical causes (disc mass, metallicity, snow and ice lines, core mass,
   stellar activity), and every change is judged on whether it keeps or widens
   the range of worlds a player can find.

This is still a simulator, not a research code: prefer the simplest plausible
model, with named and bounded approximations.

| Document | Owns |
| --- | --- |
| This plan | Delivery order, decisions and acceptance for G1 to G4 |
| [Planetary systems](../simulation/planetary-systems.md) | Implemented generation behaviour |
| [Composition guide](../simulation/planet-composition.md) | Implemented structure, retention and adoption |
| [Composition plan](planet-composition.md) | History of the layered solver and fallback (P1 to P4) |

## Evidence: where the two passes disagree

From the coverage report of 13 November 2026 (399 systems, 4,367 planets,
`data/tmp/composition-coverage.{md,tsv}`). "Fast" is the synchronous fallback
with its reference partition; "detailed" is the layered result.

| Measure | Result |
| --- | --- |
| Planets with a detailed result | 3,663 (83.9%); 704 stay on the fast model |
| Radius difference, rock and water route | 1.5% median, 9.7% p95, 15.3% maximum |
| Radius difference, envelope route | 2.4% median, 22.8% p95, 70% maximum |
| Sub-Neptune radius, detailed against fast | 3.7% smaller at the median, 27% smaller at p5 |
| Surface pressure differing by more than 100x | 338 of 2,221 comparable rock and water planets |
| Greenhouse warming above 300 K, absent from the fast pass | 330 planets |
| Extra gas lost in the detailed pass | 457 planets, up to 3.7% of mass |
| Labelled rocky, metal-rich or water-rich with a one-bar envelope surface | 303 planets |
| Detailed cost per planet | 304 ms mean; envelopes 755 ms median; 6.2 s maximum |

Causes, none of them numerical precision:

- **Different rules.** The fast partition exposes 0.001% of water; the layered
  solve exchanges all of it. The fast pass has no greenhouse and no blow-off.
  The fast radius uses published fits; the layered radius uses the project's
  own equations of state.
- **Order of decisions.** H/He survival is solved on the fast radius. The
  layered pass then changes the radius, so the survival equation no longer
  holds for the planet that is shown.
- **Two answers per planet.** Adopted results are session-only and search reads
  fast facts, so a planet depends on visit order and restarts.

Why the detailed pass is slow:

- The radial integration runs at 1e-10 relative tolerance, and `dry::Controls`
  rejects anything looser. Coupling closes pressure to 0.01 Pa and temperature
  to 1e-9, on a model that is a few percent accurate.
- The whole interior is solved again on each of up to 32 coupling iterations,
  although a few bar of gas barely moves the condensed radius.
- Greenhouse warming scans upward from equilibrium in 0.5% steps, each a
  column partition.

## Design rules

- **Decisions and elaboration are separate.** The synchronous stage owns every
  bulk fact listed in goal 1. The layered stage is a deterministic elaboration:
  layers, phases and profiles inside the decided mass and radius.
- **The fast path evaluates the detailed model.** It reads a table computed
  from the project's own solver, not an independent fit. Agreement is then a
  property of the design, checked by a test, instead of a hope.
- **Agreement is tested.** A census test asserts, for every planet with a
  layered result: radius within 1%, the same surface type, the same escaped
  inventory, surface pressure within 5% and surface temperature within 1 K.
  The exact bounds are set in G1 once measured.
- **Variety is a requirement, not a side effect.** No change may empty a
  region of the population that is physically plausible. See
  [variety targets](#variety-targets).
- Existing rules still hold: deterministic generation, stable identities,
  conserved inventories, labels never drive physics, and alpha status allows
  regenerating every body.

## G1: speed the layered solve and measure

Purpose: find out how fast the detailed model can be before designing around
its cost. No population change.

1. Add a per-stage timing and work breakdown to the coverage report: radial
   integration, coupling iterations, greenhouse scan, column partition, EOS.
2. Allow looser radial tolerance in `dry::Controls` and add a "survey"
   control set (target about 1e-5 relative, pressure closure scaled to the
   load, temperature to 0.01 K). Keep the current set as "reference".
3. Solve the interior once per planet where the gas load is negligible, then
   couple the column on fixed mass and radius with one correction pass.
4. Replace the 0.5% greenhouse scan with a bracketed root find that keeps its
   "coolest stable balance" meaning.
5. Report survey against reference on the 399-system probe: outcome and
   surface labels, radius, pressure, temperature and escaped fraction.

Acceptance: identical outcome and surface labels; radius within 0.1%,
temperature within 0.1 K and pressure within 1% of reference; median and p95
solve time reported. Decision recorded here: whether the speed-up makes the
G3 table optional for visits (it stays needed for search either way).

### G1 result

Same 399 systems and 4,367 planets, 20 threads, same tolerances as before.
Evidence: `data/tmp/g1/{reference,after}.{md,tsv}`, compared with
`data/tmp/g1/compare.py`.

| Measure | Before | After |
| --- | ---: | ---: |
| Total solve CPU time | 1,329 s | 385 s |
| Report wall time | 74.4 s | 21.1 s |
| Rock and water solve, median / p95 | 103 / 701 ms | 29 / 79 ms |
| Envelope solve, median / p95 | 748 / 1,534 ms | 194 / 570 ms |
| Slowest planet | 6.2 s | 2.6 s |
| Layered results | 3,663 | 3,681 |

What changed:

- **Greenhouse search.** Steps of half the excess replace the fixed 0.5% scan,
  and Illinois false position replaces what was in practice bisection.
- **Shooting.** A secant through high-side trials replaces bisection in the
  dry, water and envelope solvers: 70 trials per rock and water planet became 21.
- **Stage profile** in the coverage report, compiled into test builds only.

Agreement with the reference:

- **Rock and water route:** every outcome, surface and escape label matches.
  Largest differences: radius 3e-7 relative, temperature 1e-6 K.
- **Envelope route:** 26 of 1,325 planets change outcome. 21 that failed at
  the floating-point limit of the boundary now solve; 3 that solved now run
  out of work budget; 2 fail for a different reason. Solved radii agree to
  3e-7. The acceptance rule of identical labels is therefore missed by
  0.6% of planets, all at an existing numerical edge.

Findings that change later steps:

- **`f` is not monotone.** The guide said greenhouse warming never falls with
  temperature. It does, once all supply has evaporated. A full-excess step
  skipped the coolest balance on 6 planets, so the step is halved.
- **Looser tolerance is not adopted.** At 1e-6 the rock and water route keeps
  every label and agrees to 2e-7 in radius, but saves only a further third.
  The envelope route loses over half its solutions, because shooting from the
  centre to a 10 mbar boundary amplifies integration error past the 1 Pa
  acceptance band. One tolerance set stays; no survey mode was added.
- **Solving the interior once was not attempted.** Evaporating water changes
  the condensed inventory between coupling iterations, so it is not a pure
  load change.
- **Remaining cost** is radial integration: 57% of the rock and water route
  and 96% of the envelope route.

Decision: a median visit now costs about 35 ms per planet and 2.6 s at worst,
on one thread. That is fast enough for a visited system but not for search
across many systems, and 16% of planets still have no layered result. The G3
table stays required. Two follow-ups are added to G3: condition the envelope's
outer boundary so it does not need 1e-10 integration, and tabulate the EOS
calls that dominate each trial.

### Envelope boundary follow-up, 15 November 2026

The first follow-up is done, ahead of the G3 table. The gas is now integrated
inward from its outer boundary in log pressure and matched to a core integrated
outward ([mechanics](../simulation/planet-composition.md#mechanics-allocations-and-thermal-coupling)).
Same 399 systems, 20 threads, default tolerance. Evidence:
`data/tmp/g3-inward/{baseline,after,fine}.{md,tsv}`, compared with
`data/tmp/g3-inward/compare.py`.

| Measure | Before | After |
| --- | ---: | ---: |
| Solved envelope, median / p95 / max | 190 / 593 / 1,590 ms | 22 / 47 / 102 ms |
| Refused envelope attempt, median / p95 | 43 / 284 ms | 25 / 52 ms |
| Envelope route CPU time | 208 s | 31 s |
| Report wall time | 14.7 s | 5.6 s |
| Envelope-route planets solved | 616 of 1,353 | 638 of 1,353 |

- **Where the time went.** Each solve repeated the whole shoot 4 to 5 times to
  settle reference gravity, about 17 trials each, and the gas took about 14
  steps per H/He table cell because the table is only continuous at its knots.
  Gravity is now settled on the outer part of the shell alone, and steps end on
  the knots: about 3 per cell.
- **Agreement.** All 616 earlier solutions remain, with identical surface
  labels. Radius differs by 1.7e-10 at the median and 8e-7 at most; the new
  solve agrees with itself at a five-times tighter tolerance to 3e-12, so the
  larger differences are the old 0.01 mbar endpoint band. Rock and water rows
  are unchanged.
- **22 more solve**: 12 that ran out of work budget, 2 at the old floating-point
  limit, the rest at domain edges the old shooting path could not reach.
- **Default tolerance is now 1e-8** (was 1e-10), for every route. Against a
  2e-11 run no solution is lost and radii stay within 1.5e-10 (envelope) and
  7e-11 (rock and water). G1 lost over half the envelope solutions at 1e-6.

  | 399 systems, 20 threads | 1e-10 | 1e-8 |
  | --- | --- | --- |
  | Solved envelope, median / p95 | 22 / 48 ms | 9 / 20 ms |
  | Rock and water, median / p95 | 8 / 24 ms | 4 / 14 ms |
  | Refused envelope attempt, median | 25 ms | 11 ms |
  | Total solver CPU | 91 s | 52 s |
  | Report wall time | 5.6 s | 3.5 s |

  - **Cost.** The tests' independent quadrature of stored profiles closes region
    mass and inertia to 8e-8 at worst, not 1e-8, so those bounds are now 1e-7.
    Suspended trace condensate mass moves by 0.2% against the refined run (bound
    now 0.5%); it is a few millionths of the gas.
  - **One planet changes outcome**: `Bulge:52#B/4` solves at 1e-8 and 1e-6 but is
    refused (near-critical suspended range) at 1e-10 and tighter. Its radius
    agrees between 1e-8 and 1e-6 to 3e-9.

Still open from G1: tabulating the EOS calls. Whether visits still need the
envelope table at these costs is for G3 to decide; search does.

## G2: one place decides the bulk facts

1. Move greenhouse temperature, full water exchange and unbound-column
   blow-off into the synchronous stage, on the stage's own mass and radius.
   The layered solve receives the final inventory and adds no escape.
2. Use one water exposure rule in both stages.
3. Solve retention against the same radius the planet will finally have
   (the table in G3; until then, the fast radius with the G2 corrections).
4. Derive descriptions from the decided surface: a body whose surface is a
   one-bar envelope is not labelled rocky.
5. Add the agreement census test from the design rules to `check.sh` at a
   size that stays within its time budget, with the full probe opt-in.

Acceptance: the agreement test passes; no layered result reports blow-off;
the 303 mislabelled planets are gone. **Met**, with radius and
supply-limited pressure left to G3 as planned.

### G2 result

4,367 planets (57 systems per component), layered solve run on every planet
with generation's decision (`data/tmp/g2/`).

| Measure | Before G2 | After G2 |
| --- | ---: | ---: |
| Extra gas lost in the detailed pass | 457 planets | 0 |
| Greenhouse warming above 300 K missing from the fast pass | 330 planets | 0 |
| Surface temperature moved by the detailed pass, rock and water | up to 1,237 K | 0 |
| One-bar temperature moved, envelopes, p95 / max | 5 K / 82 K | 1.3 K / 11 K |
| Surface pressure differing by more than 100x | 338 planets | 0 (largest factor 1.6) |
| Surface kind decided by the fast pass | never (unresolved) | always; 1 planet changes |
| Bulk label on an H/He envelope surface | 303 planets | 0 |
| Radius difference, rock and water, p95 / max | 9.7% / 15.3% | 8.8% / 11.3% |
| Radius difference, envelopes, p95 / max | 22.8% / 70% | 31% / 53% |
| Synchronous generation, per planet | 0.09 ms | 0.20 ms |
| Layered solve, total CPU | 385 s | 274 s |
| Layered solve, median per planet | 36 ms | 12 ms |
| Planets with a layered result | 84.3% | 81.6% |

"Before" is the evidence table above and the G1 result; the two temperature
rows are from a 1,544-planet sample taken just before the change.

What changed:

- **One decision.** `structure::fallback::decide` runs after long-term escape:
  surface state, blow-off, then a `Decision` (thin surface with its
  temperature, envelope, or refused). The layered solve takes it as input,
  attempts only that route and removes nothing. See the
  [composition guide](../simulation/planet-composition.md#synchronous-surface-decision).
- **One exposure rule.** All water and every unprotected secondary exchange
  with the column in both stages. The `1e-5` water fraction now only limits
  long-term escape.
- **Protected secondaries sit in the rock** in the fast stack too, so the
  fast surface is rock, water or frost, as the layered one is.
- **Labels.** H/He that alone holds a one-bar level is a Sub-Neptune however
  little of the mass it is: 495 planets (11%) moved from rocky, water-rich or
  metal-rich. A thick secondary atmosphere keeps its bulk class.
- **Gas giants and envelopes** report the one-bar temperature of the same
  intrinsic-heat profile the layered envelope uses.
- **Column maths 20x cheaper** (series and Gauss quadrature, bracketed root),
  which is what makes the decision affordable synchronously. It also removed
  the greenhouse stage from the layered solve.

Findings:

- **Dense columns were the hidden disagreement.** 174 planets (4%) balance
  their greenhouse on a column denser than the layered mask (median 348 bar
  and 1,160 K: Venus-like and steam worlds). A trace of H/He used to send
  them to the envelope solver, which returned a cold one-bar envelope; with
  no H/He they were refused and shown cold with no greenhouse. They now keep
  the decided hot surface, and the layered solve refuses them. That is the
  2.7 point drop in layered coverage.
- **Greenhouse matters widely.** 1,491 of 3,125 decided surfaces warm by more
  than 1 K, 476 by more than 300 K. 452 planets lose gas to blow-off.
- **Radius is now the only bulk fact the stages disagree on:** up to 11% on
  rock and water worlds, up to 53% on envelopes. Supply-limited pressure
  follows it (up to a factor 1.6), and so does 1 frost case in 4,367.
- **Navigation exclusions grew.** Planets whose drag clearance exceeds their
  radius rose from 13% to 21% (ten radii: 4% to 10%), because warm, light
  columns are now present from the start instead of after adoption. A close-in
  planet of that kind can refuse a ship standoff.
- Retention still solves H/He survival on the fitted radius (step 3). That
  moves to the table in G3.

Not done, moved to G3 with the table: water phase (ocean or ice) in the
synchronous record, and the envelope solver's 1e-5 water exposure at one bar.

Candidate follow-up, not scheduled: a stricter blow-off criterion than the
static sonic-radius guard, so marginal hot columns escape instead of staying
as very extended trace atmospheres.

## G3: the fast path reads the detailed model

1. Add an offline preparation step, in the style of
   `scripts/prepare-hhe-density.py`, that runs the reference solver over a
   grid and writes a checksummed table:
   - condensed bodies: radius and inertia over mass, metal share and water
     share, with a temperature axis only if G1 shows it matters;
   - envelopes: radius over core mass, envelope share, equilibrium
     temperature and age.
2. Replace `structure/fallback/fits.rs` with interpolation in that table.
   Cells the solver refuses are filled from a smooth labelled surrogate and
   flagged, so the result stays total.
3. Retention and search use the table. Their answers match the layered model
   to interpolation error.
4. **Remove the swap** (replaces persistence, decision of 16 November 2026).
   Originally: persist layered results to `data/` so a restart does not
   revert adopted planets. Dropped: a re-solve costs milliseconds and once
   nothing visible changes there is nothing to revert. Instead, in order:
   1. Inventory every field adoption replaces and who reads it (drawing,
      shape, atmosphere, gravity, navigation clearance, inspector).
   2. Tabulate envelope inertia, so flattening is final at generation.
   3. The layered solve receives the atmosphere, it does not solve one
      (decision of 16 November 2026). Generation's pressure, temperature,
      gas mix, frost and sea are its boundary conditions, as the surface
      temperature, escape and surface kind already are since G2. Discarding
      a second, slightly different answer would hide the disagreement; this
      removes it. Detail under "Next: step 4.3" below.
   4. The layered result becomes an attachment to the planet's record, read
      by the inspector alone. Attaching it needs no barrier.
   5. Delete the adoption machinery: the update barrier, the deferral rules
      (craft fields, active routes, camera state and distance), the adopted
      projection replay and the ready, deferred and applied states. Checked
      interactively in the client.
5. Decide whether refusals still need a separate label once the surrogate
   cells exist.
6. **Decide whether the client runs the layered solver at all** (raised 16
   November 2026). If the inspector's interior view can be read from tables
   (layer boundary radii, central pressure and temperature, a few values per
   cell), the solver becomes an offline tool that builds the tables and the
   reference the tests compare against, and nothing is solved in the client.
   - For: no background workers or candidate cache; the client no longer
     needs the local H/He resource; refused planets lose nothing, since the
     solver refuses them at run time too.
   - Against: a profile is a curve, not a number, so the interior view
     becomes a few boundaries, not a full pressure and temperature run;
     envelope cells are built over one reference core, so a body's own
     core layers would be approximate; each tabulated value adds about
     2 MB.
   - Decide after step 4.1 shows what the inspector really needs. Step 4 is
     required either way and is not blocked by this.

Acceptance: agreement test passes with the table in place; search and the
inspector report the same bulk facts before and after a visit and across a
restart; fallback fit code is removed; no drawn or simulated value changes
after generation and the adoption machinery is gone. **Met for condensed
surfaces up to the swap. Envelopes miss the 1% radius target** (see the
result).

### G3 result, 16 November 2026

Steps 1 to 3 are done. Current behaviour is in the
[composition guide](../simulation/planet-composition.md#structure-table). Same
399 systems and 4,367 planets; evidence in `data/tmp/g3-table/`.

| Measure | After G2 | After G3 |
| --- | ---: | ---: |
| Radius difference, rock and water, p50 / p95 / max | 1.4% / 8.8% / 11.3% | 0.03% / 0.11% / 0.26% |
| Radius difference, envelopes, p50 / p95 / max | 3.5% / 31% / 53% | 0.36% / 2.3% / 11% |
| Envelopes within 1% / 2% | not measured | 79% / 93% |
| Surface pressure, rock and water, largest factor | 1.6 | 1.01 |
| Surface kind changed by the layered solve | 1 | 0 |
| Inertia factor difference, rock and water, max | not measured | 0.8% |
| Planets whose radius changes on a visit | most | 0 of 3,599 |
| Synchronous generation, per planet | 0.20 ms | 0.22 ms |

What changed:

- **One embedded table** (`structure/table.rs`, 2.6 MB then, built in ten minutes by
  `scripts/prepare-structure-table.sh`): condensed bodies, cores under an
  envelope, and envelopes. It is compiled in, not a local resource, so
  generation does not depend on who has the H/He data.
- **The fits are gone from generation.** `fallback/fits.rs`, the Seager
  coefficients and their parameters are removed. The envelope closed form
  survives only in the table build, as the surrogate for refused cells.
- **Retention** solves H/He survival on the tabulated radius, through the same
  fallback.
- **A visit keeps the radius.** The layered profile is stretched onto
  generation's radius at refinement. Decision of 16 November: rescale, do not
  leave the profile short or long of the displayed radius.
- **Cores under envelopes are simplified**, by decision of the same day: no
  core-composition axis. The table holds gas thickness over one reference
  core, and the body's own core radius is added.

Findings:

- **Rock and water needed a temperature axis for water only.** Dry bodies
  already agreed with the fit to 0.4%; water was the whole 11%.
- **Inverse radius is the quantity to tabulate for envelopes.** It is linear
  in temperature and in the logarithm of gas mass. Tabulating the logarithm of
  the radius on the same grid left three times the error.
- **Core composition is not a small effect.** Ignoring it, or scaling by the
  bare core radius, left envelope radii up to 20% out. Adding the core's own
  radius to a tabulated thickness is what reaches 0.36%. The remaining error is
  wet cores above 20 Earth masses and traces of H/He under heavier gas.
- **The layered model inflates hot young giants**, and the fit had hidden it:
  one 200 Earth-mass planet at 1,100 K and 14 Myr is 27 Earth radii (11 by the
  fit). Two population bounds in the tests were raised for this. Farther out
  the solver stops where such gas is unbound. G4 item 1 should remove those
  envelopes instead of leaving them large.
- **Only a third of envelope cells solve**, most of the rest in corners no
  planet occupies. The real gaps are gas giants (melting screen) and anything
  colder than about 100 K. Those planets read the anchored surrogate, and
  nothing checks it. (Since widened: see solver coverage below.)

### Solver coverage, 16 November 2026

The envelope solver now reaches gas giants and cold envelopes, so the table is
solved there instead of filled. Evidence in `data/tmp/g3-coverage/`.

| Measure | Before | After |
| --- | ---: | ---: |
| Census planets with a layered solve | 3,599 (82%) | 3,846 (88%) |
| Refused: cold H/He | 265 | 33 |
| Refused: helium melting screen, core pressure ceiling | 63 | 0 |
| Refused: ion-quantum limit | 0 | 13 |
| Envelope table cells solved | 32.5% | 65.1% |
| Jupiter and Saturn analogues | refused | 11.4 and 9.8 Earth radii (real 11.2, 9.4) |
| Envelope radius difference, p50 / p95 / max | 0.36% / 2.3% / 11% | 0.49% / 3.4% / 6.6% |

What changed:

- **Cold gas** below 100 K is solved from 40 K with a hydrogen virial term,
  where only near-vacuum was accepted.
- **Helium** below its pure melting curve is treated as dissolved in fluid
  hydrogen. The pure curve had refused nearly every cold giant.
- **The ion-quantum limit** applies only to the dense plasma it describes.
- **Pressure ceilings**: the H/He resource now spans 10,000 Mbar and
  1,000,000 K (`NSHHE002`, regenerate with `scripts/prepare-hhe-density.py`),
  and cores under an envelope use the rock and metal fits' full range.
- **Four more gas-share knots** above 0.9: giants were 5 to 7% out between
  the old ones.

The median and 95th percentile rose because the census now counts 247 planets
it could not check before. Still refused: the secondary-gas condensate limits
(253), which are now the largest envelope gap, and gas under 40 K.

Still open in G3:

In this order, towards deleting the swap (step 4):

- **Step 4.3**, the layered solve receives the atmosphere (next, below).
- **Steps 4.4 and 4.5**, the layered result as an inspector-only attachment,
  then deleting the adoption machinery.
- **Step 6**, whether the client still runs the solver.
- **Step 5**, whether refusals keep a separate label.
- Carried from G1 and G2: EOS tabulation, water phase in the synchronous
  record, and the envelope solver's 1e-5 water exposure at one bar.

### Swap inventory, envelope inertia and cores under load, 16 November 2026

Steps 4.1 and 4.2, and the giant accuracy item. Evidence in
`data/tmp/g3-inertia/`.

| Measure (envelope route, census) | Before | After |
| --- | ---: | ---: |
| Radius difference, p50 / p95 / max | 0.49% / 3.4% / 6.6% | 0.24% / 2.3% / 6.5% |
| Within 1% / 2% | 70% / 85% | 79% / 92% |
| Inertia factor difference, p50 / p95 / max | 6.2% / 14% / 58% | 0.37% / 4.0% / 11% |
| Over 25 Earth masses, radius, p50 / max | 1.8% / 5.8% | 0.9% / 3.4% |
| Under 100 K, radius, p50 / max | 1.3% / 5.8% | 0.9% / 6.5% |
| Table resource | 2.6 MB | 6.3 MB |

What changed:

- **Cores are tabulated under load**, to 1,200 Earth masses, with their
  inertia factor. Envelope cells keep the pressure at the base of the gas and
  the gas's own inertia factor.
- **A body's own core replaces the reference core at that pressure**, moved
  for the core's size. Two fitted exponents; see the composition guide.
- **Envelope inertia is composed** from the tabulated core and gas, so an
  enveloped planet's flattening is the layered solver's at generation.

Tried and rejected: conserving the gas's volume, or its depth in inverse
radius, around the body's own core. Both were several times worse than
adding the core difference.

Still loose: cores of more than a twentieth water under deep gas come out
1.5 to 3% large, since the gas thickness is the reference core's. A water
axis on the envelope grid would fix it at about three times the table.

**What the swap still changes** (step 4.1; `Refinement::project` replaces
mass, radius, inertia factor, the physical record, the result and the
atmosphere):

| Value | Thin route | Envelope route | Read by |
| --- | --- | --- | --- |
| Radius, mass, density, class, material totals | same | same | everything |
| Inertia factor | 0.2% | 0.4% (11% at worst) | flattening: drawing, clearance |
| Reference gravity | same | differs: enclosed mass at one bar | atmosphere column, clearance |
| Condensed radius and layer radii | differs | differs | ground shape, descent, clearance |
| Surface pressure | up to 1% | same | atmosphere, optics |
| Surface temperature | same | about 1 K | atmosphere scale height |
| Gas reservoir split and mole fractions | differs | differs | atmosphere, optics |
| Surface knowledge, layers, models, notes | differs | differs | inspector only |
| Provenance and sequence | always | always | inspector, ground chunk key |

- Nothing shown is a continuous profile: each layer has radii, pressures,
  one temperature and masses. Step 6 would lose nothing the inspector shows.
- The sequence bump rebuilds ground chunks even when nothing visible moved.
- The machinery to delete is about 830 lines of client code and 1,000 of
  tests, plus `refinement.rs`, `rescale` and the replay in the voyage cache.

### Next: step 4.3, the layered solve receives the atmosphere

Not started. Written as a handover.

**Why.** Generation decides the atmosphere with the layered solver's own
rules on the tabulated radius. The layered solve then reruns those rules on
its solved radius, a fraction of a percent different, so pressure, gas split
and gravity come out slightly different, and `prepare_refinement` rebuilds
`environment.atmosphere` from them. Two calculations of one thing cannot
agree exactly. One must decide and the other receive.

**Where the routes split** (`fallback/decide.rs`, `prototype::Decision`).
Thin is tried first. It becomes `Envelope` when the thin column refuses and
any H/He is present:

- H/He, or all gas together, above 1% of the planet's mass
  (`coupled::MAX_GAS_MASS_FRACTION`);
- a column beyond the ideal-gas density mask whose H/He alone holds one bar
  (`light_gas_holds_one_bar`); without that it stays a thin, dense column;
- condensed mass outside 0.0001 to 20 Earth masses.

Not checked: whether radius and pressure are continuous across the 1% line.

**Work, in order.**

1. **Measure first.** `coverage/agreement.rs` compares radius, inertia,
   surface temperature and pressure. Add, per route: gas mole fractions (the
   largest absolute difference), condensed radius, reference gravity, and
   the atmosphere record's scale height and extent. Run
   `NS_AGREEMENT_SYSTEMS=57 cargo test --release -p space-simulation generation_agreement -- --nocapture`
   and keep the report in `data/tmp/`. This says where generation's rule is
   visibly simpler than the solve. The expected case is the envelope gas
   mix: generation uses the reference partition at the equilibrium
   temperature (`partition::resolve`), the solve allocates suspended
   condensate by solved region (`physical/layered.rs`, the reservoir split).
2. **Thin route.** `Decision::Thin` already carries the surface temperature.
   Extend it, or the request built in `prototype::thin_request`, with the
   decided gas inventory, frost and surface pressure, and have the coupled
   solve integrate the interior under that load without repartitioning
   (`coupled.rs`, `coupled/column.rs`, `coupled/surface.rs`). The solved
   radius still differs from the table's by interpolation, and
   `layered::rescale` still stretches the profile onto generation's.
3. **Envelope route.** The gas shell is part of the structure, so it stays
   solved. Give it generation's gas mix as fixed, or move generation to the
   solve's rule if step 1 shows generation's is the poorer one. The one-bar
   temperature is the same closed form on both sides already
   (`fallback::one_bar_temperature_k`). Reference gravity differs only
   because `rescale` divides it by the stretch squared: use `GM/R²` on
   generation's radius.
4. **Stop rebuilding.** `prepare_refinement`
   (`physical/refinement.rs`, the block that builds `GasAtmosphere` from
   `gas_kg`) keeps the initial `environment` unchanged, and the refined
   record carries generation's surface pressure, temperature, gravity,
   condensed radius and reservoir split. `validate_refinement`
   (`physical/result.rs`) should then require them equal, as it does mass.
   The five census planets whose refined optics are unsupported
   ("refined cloud/optical inputs unsupported") then install normally.
5. **Stop the needless rebuild.** `ground_key` (`client/src/scene/draw.rs`)
   hashes the result's sequence, so ground chunks rebuild on every swap.
   Once nothing it draws from can change, drop the sequence from the key.

**Acceptance.** On the census, a refined planet's atmosphere record,
reference gravity, condensed radius and gas split are bit-identical to
generation's, on both routes; the agreement test asserts it. Bump the
`layered_adoption` part of the generation key in `system/forcing.rs`.

**Working notes.**

- Run `./scripts/check.sh` after code changes. Cap CPU use at 16 threads:
  `CARGO_BUILD_JOBS=16 RUST_TEST_THREADS=16`, and `NS_TABLE_THREADS=16` for
  a table build.
- Step 4.3 needs no table rebuild and no new cell values. If the envelope
  gas mix must be tabulated after all, it is one more value per envelope
  cell (see `table/build.rs`, `Solved`), nine minutes to rebuild.
- The table build and the envelope tests need the local, git-ignored
  `data/composition-sources/hhe-density.bin` (`NSHHE002`), from
  `scripts/prepare-hhe-density.py`.
- Client behaviour (steps 4.4 and 4.5) is checked with the user, in the
  running game.

Then step 4.4: `Refinement::project` writes nothing a consumer draws from
or simulates with, so the layered record becomes an attachment the inspector
reads, and step 4.5 deletes the barrier, deferral rules and replay.

## G4: formation priors and variety

Each item is a separate change with a before and after census. The table
from G3 depends only on structure physics, so none of these needs it rebuilt.

1. **Gas loss.** Replace the smooth survival curve, which never reaches zero,
   with energy-limited loss integrated over the host's activity history, and
   complete stripping below a boil-off floor. Expected: bare inner rocky
   worlds, a radius valley, and surviving envelopes farther out.
2. **Gas accretion.** Replace the 5.5-decade log-uniform ratio with a critical
   core mass that falls with distance. Below it: envelopes of a fraction of a
   percent to tens of percent, scaling with core mass and disc lifetime.
   Above it: runaway to a giant, limited by the disc.
3. **Disc budget.** Give each host a solid and gas budget scaling with host
   mass and a new metallicity input by galactic component and age, with wide
   scatter. Planet masses are drawn from the budget, which correlates
   neighbours and removes the reversed host-mass trend (median 0.146 Earth
   masses under 0.5 solar masses against 0.082 for Sun-like hosts).
4. **Planet count and architecture.** Replace the uniform 7 to 14 with a count
   that follows the budget and spacing. Draw an architecture per system
   (for example compact chain, mixed with outer giants, giant-dominated,
   sparse and wide, debris-poor remnant) as a continuous prior, not a class
   template. Remove the mass-floor pile-up from the compact retry.
5. **Refractory mix.** Centre the metal share on a per-system value from host
   abundance, with rare stripped-mantle and iron-poor outliers kept.
6. **Volatile mix by distance.** Use ice lines for CO2, NH3, CH4, CO and N2,
   with the volatile share rising outward.
7. **Dynamics.** Tie eccentricity to multiplicity and neighbouring giants;
   make close-in giants rare and migration-labelled; let giants clear gaps.
8. **Spin.** Make tidal locking depend on age and planet properties.

### Variety targets

Realism must not flatten the population. Each G4 census reports the share of
these outcomes, and a change that removes one needs a stated reason:

- Bare hot rock, lava worlds and stripped cores close in.
- Airless rock, thin CO2 or N2 air, thick Venus-like air, steam atmospheres.
- Ocean worlds, ice shells over oceans, deep-ice worlds, frost-covered rock.
- Cold nitrogen and methane worlds beyond the outer ice lines.
- Iron-rich and iron-poor bodies as outliers, not a fifth of all planets.
- Sub-Neptunes on both sides of the radius valley, low-density puffy worlds.
- Ice giants, gas giants, hot and warm giants, very massive giants.
- Systems of different kinds: tight chains, giant-dominated, sparse, empty.

Variety is measured, not assumed: the census prints these shares per galactic
component, host mass band and age band, with one named example ID for each.

## Out of scope

Moons, rings, belts as bodies, N-body evolution, atmospheric chemistry and
climate. Terrain and surface rendering follow this plan, not the reverse.

## Risks

- Looser tolerances can move bodies across phase seams. G1 compares labels,
  not just numbers.
- A table adds a prepared resource to the build. It must be reproducible from
  the repository and checksummed like the H/He resource.
- G4 changes every generated body. That is allowed in alpha, but each item
  lands separately so its effect on the census can be reviewed.
