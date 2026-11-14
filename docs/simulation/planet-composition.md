# Material inventories and approximate structure

The physical foundation lives in simulation `system/composition.rs`
and `system/structure.rs`. [Planet generation](planetary-systems.md) now draws
inventories first and derives retained mass, radius and inertia from them.
Atmospheric gas and condensed reservoirs now share the same conserved inventory;
validated inventory/reference transport and descriptive labels feed the inspector.
The [replacement plan](../plans/planet-composition.md) owns remaining work and the
region/boundary contract; the [scientific reference](planet-composition-reference.md)
owns input bounds, selected/candidate sources and future acceptance requests.
Implemented radial solvers are separate headless APIs, described below.
Default generation is now authoritative on one synchronous
[labelled fallback result](#synchronous-reference-partition) per planet,
`Planet::initial_result`. Live mass, radius, inertia, reservoirs, atmosphere and
spin are all projections of it, installed together; the legacy
`structure::derive` model and boundary solver have been removed. Background
loading refines those results by default and adopts complete revisions when
[safe](#safe-background-adoption), otherwise retaining the active fallback.
The separate [water-shell solver](#headless-restricted-water-shell-solver) uses
direct [IAPWS water physics](water-physics.md) and an explicitly approximate
effective deep-ice compression model. The mandatory Earth-mass, 30%-water fixture
solves at both 250 K and 300 K. Cold deep ice is explicitly approximate rather
than resolved into individual polymorphs. The standalone
Lai ice-VII branch remains separate, not used by the radial solver.
[Condensed-boundary/atmosphere coupling](#headless-atmosphere-and-condensed-boundary-coupling)
is available through the headless solver and background refinement, never as
an expensive synchronous generation step.
[Protected secondary traces in dry hosts](#headless-protected-traces-in-dry-hosts)
and [water-bearing hosts](#headless-protected-traces-in-water-bearing-hosts)
are available as headless APIs and used by the background layered solver.

## Authoritative result contract and validation

P3's contract is implemented in shared `planet_result.rs` and simulation
`system/physical/result.rs`. Generation's synchronous result is now live; the
background loader now publishes validated candidates through a guarded adoption
barrier. The [cutover plan](../plans/planet-composition.md#p3-default-cutover-and-legacy-removal)
tracks that migration.

`PlanetResult` owns one `LayeredPlanet` physical/layer record, with typed
layered/fallback provenance and refinement status. `PhysicalRevision` carries
caller-owned immutable input and model/resource identities plus an adoption
sequence. These are not automatically computed hashes, time stamps or proof of
safe adoption. The caller must include body identity, retained supply, forcing
and resource/prescription changes in those keys. A new input/model identity
starts a new lineage; candidate work from another lineage must be discarded.

Each transported layer includes its full nonzero material inventory as material
IDs and masses, alongside the dominant-material label. Both layered and fallback
adapters copy the actual region inventories, without reconstructing composition
from whole-planet fractions. Supplied breakdowns must contain unique known IDs,
positive finite masses, and sum to the region mass. Older records may omit the
breakdown; clients show it as unavailable rather than assuming a pure region.

`physical::initial_result` computes the cheap fallback without attempting a
layered solve or manufacturing a refusal. `resolved_result` converts an actual
layered/refused outcome into a candidate. Their common validation establishes
transport consistency only. All physical fit prescriptions remain unchanged.

- Initial fallback can be pending/running. A worker failure or unavailable
  resource retains that usable result with a separate reason. A true layered
  refusal alone sets `fallback_reason`; completed layered results have no refusal.
  Provenance is explicit, never inferred from the absence of a refusal.
- `SurfaceKnowledge` is distinct from the older geometric `RadiusReference`:
  one bar is a gas reference; liquid water is liquid; ice-Ih is a modelled solid
  phase. Cold-mechanics rock, frost, unresolved water and fallback boundaries
  remain phase-unresolved. No temperature threshold invents a refractory melting
  law. **Even a known solid phase is not safe-landing permission.**
- `validate_record` checks supported model identity, finite physical quantities,
  complete unique materials, species-local source/retained/escaped/reservoir
  conservation, total and layer mass, bulk density, bounded reference gravity
  and inertia, contiguous geometry/pressure, phase allocations, boundary/surface
  consistency, reference-pressure semantics and model limitations. Gravity at an
  envelope reference is bounded by fully enclosed and total mass, not forced
  equal to exterior `GM_total/R²`. Exact local gravity or EOS/phase accuracy
  cannot be reconstructed from these transport fields.
- `validate_result` additionally checks provenance, refinement status, nonempty
  revision keys and justified surface knowledge. `validate_refinement` requires
  the same input/model identities, exactly the next adoption sequence, unchanged
  formation supply, preserved prior loss and accounted additional blow-off.
  It rejects stale/unrelated candidates without modifying either result.
- No validator re-runs a structure, EOS, retention or atmosphere-partition solve.
  Accepting an internally coherent nonlegacy radius and small positive inertia
  is intentional, not an invitation to skip independent solver regressions.
  The population census keeps its independent input-budget checks as well.

Generated `Body::initial_result` stores this result. Formation constructs it from
provisional forcing; final retention replaces it using all-star forcing and
retained supply, with its own [reference gas partition](#synchronous-reference-partition).
Classification uses its model-neutral material/mass fractions. A single
`physical::export_planet` adapter carries it through both natural and overlay
wire paths as `Planet::initial_result`, and every live field is projected from it
(see [live projection](#live-projection)).

Synchronous revision keys use exact input bit patterns for body identity, source,
retained and escaped inventories, bolometric/loss forcing, albedo and age. They
exclude task/page metadata and the mutable atmosphere reservoir split. The model
key is caller-maintained and must change with generation/retention/fallback
prescriptions. The current generation key is
`generation_physics_v2:fallback_partition_v1:layered_adoption_v2:NSHHE001-b4e7a79ecc35d846`,
covering the initial partition, refinement adapter and checksum-pinned H/He resource.
Its availability is process-local and fixed by the table's `OnceLock`: installing
or changing it requires restarting the process, not reusing an old refusal.
Background jobs capture the original result and sequence before dispatch;
initial results start at sequence zero and their refinement at exactly one;
regeneration is not an adoption or permission to renumber stale worker output.

### Live projection

Final retention installs every live field from the one prepared result, in one
step: `mass_earth` (retained mass), `radius_km` (reference radius),
`inertia_factor`, `physical` (the result's record, verbatim), the reservoir
budget, the generated atmosphere and spin (drawn with the result's inertia and
H/He fraction). No legacy structure or boundary solve runs.

- The live atmosphere is the result's own partitioned column. Positive columns
  below 0.1 Pa stay in the result (gas inventory and reference pressure) but are
  omitted from `Planet::environment`. This presentation threshold keeps the live
  population's rendering and navigation parity with the previous model; it is
  not a physical erase or an optical-depth visibility rule.
- `physical::validate(Planet)` checks transport only: the result validates; the
  live record equals it; mass, radius and inertia match; fallback thermal forcing
  matches; the atmosphere passes `OpticalColumn::reference_from_result`, or is
  omitted only below the threshold. It re-runs no solver. A record with
  `physical` but no result cannot be checked and is invalid, not trusted.
- Layered boundary temperature is independent of equilibrium forcing. Transport
  validation does not require supported optics: suspended condensates can be
  physically transported while their cloud optics and drag clearance explicitly
  refuse. No bulk condensate is silently turned into a cloud optical profile.
- Rendering, navigation clearance, shape/spin and reflectance consume the active
  result or its checked projections. The inspector prefers the loaded system,
  follows its revision and separates unapplied candidates. Background adoption
  applies those same projections through the safety barrier described below.

### Consumer boundary semantics

`system::surface::Geometry` exposes the selected reference and the outermost
non-gas region, including any frost deposit above the loaded condensed host.
Gas-reference phases remain unresolved; cold rock, known ice and liquid water
retain their distinct knowledge. None grants landing permission. The same bounded
flattening scales the reference and condensed spheroids, not a new internal figure
solution. Rendering may show a condensed top through its atmosphere, or retain a
separate [opaque visual proxy](../client/rendering.md#giant-descent) when the
bounded continuation cannot reach it. A gas coordinate is never terrain.

Clearance validates the active projections and builds its column with enclosed
reference gravity. Orbital motion/drag speed still use retained total mass.
Camera neighbourhood fields refresh when the system record changes, including
physical revision changes with the same system ID. Craft retain an immutable
field and route snapshot; adoption never replaces those snapshots mid-flight.
Their readout names the model reference used for altitude zero, not a guaranteed
solid surface. Their use pins the system against further adoption.

Search stays deterministic on synchronous fallback facts, discloses each planet's
input/model/sequence basis and flags a differing loaded basis as not revalidated.
It does not promise an exact layered match. Developer snapshots expose active
results/geometry separately from the background candidate cache.

### Safe background adoption

Background refinement is on by default. The initial synchronous fallback remains
fully usable throughout preparation, calculation, refusal, adapter failure or
deferral. Worker output includes the captured base revision, complete next result
and its atmospheric projection. At publication, the base record and immutable
forcing must still match, and every loaded copy must pass refinement validation.
Stale work is discarded, never renumbered. Changed input/model identities restart
preparation with a new cancellation epoch.

The update barrier runs after camera placement and before camera neighbourhood,
inspection and drawing consumers. It publishes at most two planets per frame,
replacing mass, reference radius, inertia, physical record, atmosphere and result
as one operation. Safety checks themselves are also limited to two per frame,
rotating through the ready queue so a deferred planet cannot starve later ones.
Identity, tasks/pages, orbital elements and spin phase/period
stay fixed; shape is recomputed from the new facts with the existing flattening
bound. The atmosphere uses the result's gas inventories/reference conditions,
not the old atmosphere rescaled to a new radius. Unsupported cloud/optical inputs
retain the active fallback with an explicit unavailable/adoption reason.

Publication is conservatively deferred:

- If any craft retains a field containing the affected planet, the whole system
  stays on its current facts, regardless of how far that craft is from the planet.
- Any active route pins all candidate publications, since routes can retain
  destination/obstacle geometry beyond the craft's current field. Completed
  routes alone do not impose this global pin. No craft is teleported or given a
  different mass field under its current trajectory/prediction.
- On the viewed physical page, the camera must be placed, not flying or in a
  layout transition, and outside twice the larger old/new bound. That bound
  encloses the spheroid, optical atmosphere top and preferred navigation
  clearance. Unavailable clearance defers rather than guesses. Camera position,
  target, orientation and speed are never corrected to make adoption fit.

Deferrals are retried while the target is loaded and refinement is enabled.
The inspector and `ns planets` distinguish ready/deferred/unavailable/applied
states. A solver refusal completes the status of the *existing partitioned
fallback*, preserving its geometry rather than installing the failed trial's
different unpartitioned fallback. Worker failure likewise retains the active
result and exposes a separate failure status.

Adopted projections are retained as session authority in the craft charts,
separate from the four-system candidate LRU and the evictable voyage cache. They
are replayed only onto the exact captured base on backend refresh/revisit, remote
inspection, craft spawning and incoming voyage replies. Late seeded worker output
cannot undo an adopted result. Rebuilding the candidate LRU reuses committed
results rather than solving them again or reporting their old base as stale.
Voyage arrival origins/pages stay fixed and its
gravity/clearance field is rebuilt as a unit. This session registry is not bounded
by the candidate LRU and is not persisted to disk; a restart begins with fallback
facts again. Turning refinement off pauses work/publication, not adopted facts.

Search remains deterministic on its synchronous basis: a differing loaded or
session-retained revision is marked **not revalidated**. It is not silently
removed/reordered or advertised as a newly certified match. Exact layered query
reconciliation and mid-flight craft-field rebasing remain separate work.

## Approximation policy and fidelity labels

The layered solver uses named, deliberately selected approximations where
reference-checked local models do not cover a required state. A failed EOS call
does **not** authorise arbitrary extrapolation or a silent fallback. Each added
route needs a defined equation, application scope, provenance/assumptions,
limitations and numerical conservation/refinement checks. Until such a route is
implemented, unsupported states still refuse. Invalid densities, unclosed mass,
unresolved numerical intervals and exhausted work budgets remain failures.

`LayeredPlanet` and each `Layer` expose `model_fidelity` and human-readable
`model_limitations`; layer radii/pressures identify where the approximation applies.
Stable model IDs remain in `physical.approximations`. Fidelity values are:

- `reference_checked`: local equations checked against reference data in scope,
  **not** whole-planet observational validation or a percentage error guarantee.
- `approximate`: a named structural, thermal or material approximation is used.
- `extrapolated`: a model is explicitly extended beyond its original mask.

The whole-planet label is no stronger than its least-supported layer or global
closure assumption. All current whole-planet results are at least `approximate`,
because they use cold structural proxies and prescribed thermal/partition
closures. Existing extended-Ih regions are explicitly `extrapolated`. No
automatically generated planet is labelled scientifically validated. The
inspector shows both global and per-region limitations; rendering is unchanged.
Numerical precision and physical uncertainty are separate: tighter integration
does not make an engineering density proxy more physically accurate. Local
reference tests remain mandatory; eventual Solar System and exoplanet comparisons
supplement them, accounting for uncertain composition, age, thermal state and
observed radius reference. Similar mass alone is not an accuracy benchmark.

## P2 envelope prescription decision probe

P2 step 2 selected the following starting prescriptions using
`scripts/probe-envelope-prescription.py`. That offline decision probe cannot
predict a planet radius or demonstrate increased coverage. The separately
implemented [coupled envelope path](#self-gravitating-envelope-and-loaded-interior)
below now integrates these choices with explicit additional bounds.

### Density and mixture

Use the original CMS19 pure-H and pure-He densities with the HG23 additive-volume
correction, rather than treating the mixture as one ideal molecular gas at all
depths. The decision-probe identity is `cms19_hg23_log_density_candidate_v1`:

```text
Y = M_He / (M_H + M_He); X = 1 - Y
v = X/rho_H(P,T) + Y/rho_He(P,T) + X Y Vmix(P,T)
rho = 1/v
```

Species inventories remain separate; X/Y are normalised within H/He, not the
whole planet. The region is uniformly mixed and excludes water/heavy material.
The sampled generator gives Y=0.25, but the mixture equation retains variable Y.
The probe interpolates pure log-density bilinearly in log-pressure/log-temperature
and the signed HG23 volume correction linearly with the same weights. No second
ideal-mixing correction, chemical conversion, helium rain or entropy-derived
thermal gradient is added.

Why this choice: a dilute molecular ideal gas cannot describe compression
through the giant's dense interior. The existing source audit provides a
stronger starting point without inventing a fitted universal compression law.
This is not a quantitative accuracy claim for the selected interpolation.

The [reference audit](planet-composition-reference.md#independent-hhe-reference-audit)
owns provenance, hashes, unit conversions and physical warnings. The decision
probe reads separately downloaded, hash-verified files; no EOS data is bundled
or redistributed. A density inspection rectangle of 100–100,000 K and
0.001–1,000,000,000 bar is **not a fluid validity domain**. The probe reports
restricted pure melting screens and corner compressibility signs, but neither
certifies mixture phases, interpolation derivatives or quantum/plasma validity.
Known invalid/unresolved states must not silently become successful runtime
envelopes. Source redistribution permission and a runtime packaging policy must
also be settled before bundling tables with the client. The runtime experiment
below instead uses a separately prepared local resource.

In particular, the cold dilute outer atmosphere below the table's 100 K minimum
needs a bounded join to the existing gas prescription. Cold dense material needs
a justified fluid mask or a separately labelled effective approximation, not
table extrapolation. The runtime path below adds a cold dilute join and bounded
screens, but cold dense and complete giant coverage still remain unresolved.

### Temperature prescription

Use a simple grey irradiated outer region and a prescribed convective-like deep
profile. For this **decision probe**, gravity is a fixed supplied reference,
not inferred from legacy radii or solved self-consistently. Its identity is
`grey_pressure_opacity_fixed_gradient_candidate_v1`:

```text
P0 = 1 bar
kappa(P) = kappa0 P/P0; kappa0 = 0.001 m²/kg
tau(P) = kappa0 P² / (2 g_ref P0)
T_rad(P)^4 = T_eq^4 + (3/4) T_int^4 [tau(P) + 2/3]
nabla_rad = d ln T_rad / d ln P
nabla_deep = 2/7
```

Locate the radiative/deep join where `nabla_rad=2/7`. Beyond it,
`T=T_join (P/P_join)^(2/7)`, giving continuous temperature and first logarithmic
derivative. A pressure-dependent opacity is deliberate: constant opacity gives
a deep radiative gradient of 1/4 and would never trigger this 2/7 join.
This is a grey diffusion closure, not the full Guillot irradiated-atmosphere
solution, opacity chemistry or a climate model. Irradiation heating, deposited
above the optically thick region, is represented by the additive `T_eq^4` term.

The 2/7 slope is the molecular-diatomic ideal-gas reference, deliberately used
as an **effective prescribed slope**, not claimed to be the true H/He adiabat.
It neglects mixture heat-capacity variation, dissociation, ionisation and
degenerate thermal behaviour. Do not read the audited table's unreliable/clamped
entropy gradients to imply otherwise. Reference opacity and gravity are further
uncertainties; full hydrostatic coupling must assess their impact.
In step 3, reference gravity must be iterated from the new-model solution,
not borrowed from legacy geometry to make the coupling appear closed.

Use `T_int=50 K` as the reference experiment, with 30/100/200 K sensitivity
cases, not an inferred age or a validated universal planet temperature. `T_int`
represents intrinsic flux (`F_int=sigma T_int^4`), not core temperature. A runtime
default is gated on coupled radius/reference tests in step 3; no cooling history
or unvalidated mass-to-age mapping is introduced. Probe forcing bounds are
20–2,000 K for equilibrium, 20–200 K for intrinsic temperature and
0.1–1,000 m/s² for reference gravity, with pressure 10⁻⁷–10⁹ bar. These are
explicit experiment bounds, not demonstrated physical accuracy bounds. Use the
existing 20 K floor on generated equilibrium inputs.

### Reproduction and acceptance evidence

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/probe-envelope-prescription.py data/composition-sources > data/tmp/p2-step2-prescription.json
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test-envelope-prescription.py
```

The standard-library probe has no network access and solves no planets. Six
synthetic tests cover thermal value/slope continuity, analytic versus numerical
gradients, heating trends, invalid inputs, log-density interpolation, pure
limits and applying the signed volume correction exactly once. They run in
`scripts/check.sh` without downloaded data.

Source-based density comparisons reproduce 0.276323 kg/m³ at 10 bar/1,000 K,
653.393 kg/m³ at 1,000,000 bar/10,000 K, and 1,842.102 kg/m³ at 10,000,000
bar/19,952.623 K for Y=0.25. The 1,000,000 bar/5,011.872 K negative fixture still
reports the pure-He melting warning; finite density does not override it.
These reproduce the source construction, not independent EOS or planet-radius
accuracy measurements.

At reference intrinsic 50 K, the 20 K/10 m/s² cold scenario joins at 0.432 bar,
the 250 K/10 m/s² scenario at 14.91 bar, the 110 K/25 m/s² giant scenario at
4.612 bar and the 1,074 K/10 m/s² hot scenario at 275.12 bar. This verifies that
irradiation changes the depth of the prescribed radiative region. It does not
establish those boundaries for generated planets. The cold scenario leaves the
table rectangle in its outer region, and cold/giant reference deep paths can
trigger pure-He melting warnings: the probe intentionally exposes these gaps.

**Step-2 stop condition:** prescriptions and equations selected, sensitivity
inputs explicit, analytic checks passing, and unresolved table/phase/thermal
assumptions recorded. Step 3 must validate the resulting whole-planet structure
before selecting runtime defaults or claiming a percentage accuracy.

## Self-gravitating envelope and loaded interior

`structure::envelope` implements `self_gravitating_loaded_hhe_v1` through the
same adaptive radial kernel as the dry/water solvers. It is used by the background
prototype after a thin-route gas-mass, density or heavy-mass refusal on an
H/He-bearing body. Successful thin solves are returned unchanged. There is no
legacy radius, atmosphere or gas-search cache in the new solve. Default generation
uses the synchronous new-model fallback and its live projections; this layered
envelope route may replace it through guarded background adoption when its
transport, optical adapter and safety checks all succeed.

### Local data, not a redistributed table

The runtime resource is generated locally from the existing separately held,
SHA-256-pinned CMS19/HG23 originals:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/prepare-hhe-density.py data/composition-sources data/composition-sources/hhe-density.bin
```

No source dataset or converted resource is committed, downloaded automatically
or bundled with the client. This keeps unresolved upstream redistribution
permission separate from local experimentation, rather than interpreting a
third-party metadata licence as original-author permission. A distributable
data package is still future work. `NS_HHE_DENSITY_PATH` can point to a prepared
file outside the development default path. Each process loads it once;
preparing/replacing it after a failed load requires restarting that process.
Missing or corrupt data produces an explicit unsupported error, never a
substituted EOS or legacy solve. Standard project tests need no source files.

The 398,041-byte little-endian v1 file contains 61 temperature knots and 261
pressure knots, storing two original log densities, signed HG23 volume
correction and pure compressibility flags. SHA-256 of the deterministic output:
`8237f7f2b20457553d54df10c63ab069fd6bdc5b0e133e38c6ec6c9308c5f9fb`.
Runtime schema/finite-value checks and pinned FNV-1a corruption checksum
`b4e7a79ecc35d846` supplement the converter's source SHA checks. FNV is not
cryptographic source authentication.

### Mechanics, allocations and thermal coupling

- Integrate `dr/dm`, `dP/dm` and `dI/dm` from the regular centre through metal,
  silicate, water (if positive), then the H/He parcel. The actual enclosed mass
  includes every parcel, so envelope self-gravity and interior compression are
  solved together. Shared interface radius, pressure and enclosed mass are
  identical by construction. Inertia comes from RK integration, including the
  cumulative interior contribution, not a fitted core or shell formula.
- Internal carrier coordinates are not origin identities: the heavy parcels
  include protected secondary mass, while the outer carrier owns the complete
  H/He mixture and accessible secondary traces. Restore separate inventories
  before returning and check closure for each origin at relative 10⁻¹⁰. Keep
  generation's prior escaped inventory unchanged; no retention history is
  replayed and no gas is silently discarded.
- Protected secondaries retain P1's host-volume prescription. Accessible
  secondaries up to 0.301% of envelope mass retain the original H/He host specific
  volume under `passive_secondary_envelope_host_volume_v1`. This is a mechanical
  trace approximation, **not** protection, dissolution or condensation physics.
  Water is never folded into that trace allowance or dropped at a threshold.
  Above this bound the [bounded molecular extension](#hhe-rich-molecular-mixtures)
  is attempted, not a widened passive trace approximation.
- Water retains separate regions. Above 10,000 bar,
  `loaded_effective_water_compression_v1` uses the existing Seager pressure-only
  water row, through 100,000,000 bar. Below that a separate
  [shallow loaded-water proxy](#shallow-loaded-water-approximation) applies.
  Their phase is `UnresolvedCompression`, not
  ice or identified fluid; thermal expansion, internal heat transport and
  mixing are unresolved. Its displayed temperature is the shared outer-boundary
  temperature. The original local water EOS masks and thin solvers are unchanged.
- The thermal form selected above is prescribed, not a cooling calculation.
  Generation sets the intrinsic temperature from retained mass and age,
  `T_int = 100 K (M/M_Jupiter)^0.215 (age/4.57 Gyr)^-0.1`, clamped to
  20–200 K (`envelope/thermal.rs`). The mass exponent joins Jupiter (about
  100 K) and Neptune (about 53 K) today, with Saturn then at its measured 77 K;
  below Neptune's mass it is an extrapolation. The age exponent is deliberately
  far gentler than giant cooling luminosity implies (about -0.3): that value
  inflated young low-mass envelopes to several times the retained fit's radius.
  It replaced a fixed 100 K for every body, which
  [inflated old low-mass envelopes](#intrinsic-temperature-census). The headless
  API still accepts 20–200 K for sensitivity work. Reference gravity is
   iterated with bounded secant updates from newly integrated enclosed mass at
   the radius reference (one bar, or the condensed boundary for a sub-bar gas
   shell). Require relative gravity closure below 10⁻⁸. No legacy radius seeds
  this fixed point. Molecular heat-capacity variation and true deep adiabats
  remain unresolved, so radius accuracy is not established.

The new full-planet route admits 0.0001–1,200 Earth masses and up to 100 Earth
masses of heavy inventory, but never widens the refractory pressure ceiling of
100,000,000 bar. Both a positive heavy host and positive H/He supply are required;
core-free pure-gas spheres are not an accepted input. These are request bounds, not a guarantee all such bodies
solve. The existing standalone dry/water 20-Earth-mass contracts stay unchanged.

### Density restrictions and finite outer boundary

Use log-bilinear pure densities plus the signed mixture correction once.
Require positive corner compressibilities, positive mixed specific volume and
positive interpolated mixture compressibility. Returned profiles also reject
outward density inversions. No table entropy, cp/cv or supplied adiabatic
gradient is used.

The density-table runtime bounds are 100–100,000 K and **0.01 mbar to
100,000,000 bar**. The cold dilute prescription below is a separate extension.
Density-table bounds alone are not a validity mask. The runtime applies:

- CMS19 H₂ melting screen up to 3,000,000 bar; beyond that, require a hot path
  of at least 10,000 K rather than extrapolating the turning molecular curve.
- The CMS19 pure-He Simon melting screen, conservatively **continued as an
  engineering restriction** through the bounded pressure rectangle. This
  continuation beyond the audit's restricted experimental comparison is named
  `bounded_pure_he_melting_screen_extension_v1`, not presented as a validated
  mixture freezing curve. It can exclude real fluid mixtures and remains a
  known giant-coverage limitation.
- CMS19 equation 3's ion-quantum temperature screen for each present pure
  endpoint, with source cgs density units converted explicitly.
- Below 100 K, `dilute_hhe_100k_volume_anchor_v1` uses the molecular ideal
  specific volume plus the table's 100 K nonideal volume residual. It is
  continuous in value at 100 K and allowed only when ideal density is at most
  0.1 kg/m³, down to 20 K. Cold dense inputs are refused, not extrapolated.

These are labelled conservative restrictions, not complete mixture-phase
thermodynamics or statistical accuracy bounds. Unsupported phase/quantum/table
or heavy-water conditions remain errors; no successful partial profile is
returned. Mathematical shooting trials may use the pressure-only water row
below its accepted depth, but a final structure outside the depth bound is
refused, never exposed as a solved water layer.

The whole retained envelope is represented in a finite shell ending near
**10 mbar**, with an operational endpoint tolerance of **0.01 mbar**. One bar is
located as an exact pressure event *inside* that shell, not used as a pressure
floor, ground or an invented material interface. Centre shooting at central
loads of tens of millions of bar cannot universally resolve a 0.0001 mbar outer
residual in f64; the new shell's endpoint contract is 0.01 mbar, recorded without pressure translation or radius
rescaling. Existing P1 tolerances are unchanged. The one-bar radius and total
inertia pass tighter-integration/half-step refinement checks at relative 10⁻⁷
in the source-backed small-envelope fixture. More extreme ill-conditioned
shoots explicitly fail rather than loosening their acceptance further.

The radius and inertia factor use the one-bar reference when it exists. If the
gas base is at less than one bar, they use the shared condensed/gas boundary
instead: `one_bar_radius_m` is absent, while reference pressure and temperature
are the actual boundary values. Reference gravity uses the mass enclosed at
the selected reference. A base exactly at one bar uses that boundary as the
one-bar reference; a thicker shell without a located one-bar event still fails.
No interpolation, new pressure cutoff or extrapolation into the interior is used.
The gas shell retains its full inventory, profiles and inertia above the
reference. The selected condensed surface keeps its existing phase knowledge:
unresolved compressed water is not labelled as solid ground. The existing
10 mbar shell endpoint and all EOS/closure gates remain unchanged.

Integration remains at relative
10⁻¹⁰ by default, with bounded 32 gravity iterations, 128 shooting iterations,
2 million EOS evaluations per radial solve and 16 million across the request.
An independent sample quadrature checks total inertia within 0.1%; it is not
the delivered inertia and does not relax integration tolerances.

Invalid intermediate RK volumes or non-finite stages reject the proposed mass
step and retry at one quarter of its size, without altering the accepted state.
These retries consume the existing step/EOS budgets. Failure to recover before
the minimum step is a numerical refusal, never evidence that central pressure
is too low. EOS/domain errors and exhausted budgets propagate unchanged; phase
events and the pressure-coordinate bridge retain their existing contracts.
This changes no physical prescription or accuracy tolerance.

The step-recovery checkpoint is **1,416/1,544 layered (91.7%)**, with all 128 remaining
bodies on the checked fallback. It recovers 35 of 36 radial refusals and one
previous pure-melting-screen refusal whose shooting path had been cut short by
an invalid stage. All 1,380 previous successes retain exactly identical full
transport records and non-timing census fields. Mock-EOS regression tests cover
rejection/retry, EOS error propagation and work accounting; two recovered
source-backed bodies pass radius/inertia refinement at relative 10⁻⁷. One
outer-envelope minimum-step failure remains unresolved.

Sub-bar references raise the census to **1,437/1,544 layered (93.1%)**, with all
107 remaining bodies on checked fallbacks. All 24 formerly unresolved one-bar
cases genuinely start below one bar (initial base pressures 0.011–0.857 bar).
Twenty-one now solve; three expose a separate region-mass resolution refusal.
All 1,416 previous full transport records and non-timing census fields are
exactly unchanged. Tests distinguish sub-bar, exactly-one-bar and missed-event
cases, verify actual boundary radius/pressure/temperature/gravity, and check
two recovered water-bearing bodies under tighter integration at relative 10⁻⁷.

### P2 final validation

The coverage probe now independently checks every layered and fallback transport
record against its generated budget: all constituent source/retained/escaped
and reservoir allocations, prior escape plus additional blow-off, total layer
mass, finite positive physical quantities, ordered radii/pressures, central and
shared boundaries, phase suballocation bounds, provenance and JSON round trips.
Fallback reasons must be present and layered results must not carry one. Shared
thin-boundary pressures retain their existing absolute 0.02 Pa numerical
contract, including nearly airless surfaces; this is not a new solver tolerance.

`source_backed_mass_heating_and_stripping_trends` adds 17 full-envelope
experiments using the local pinned density resource. At 0.1% gas/heavy mass,
1–10 Earth masses of heavy material give increasing radius and dimensional
inertia. A fixed 5-Earth-mass heavy interior plus 0.01 Earth masses of gas
expands from 11,886 to 13,542 km under equilibrium heating from 250 to 750 K,
and from 11,503 to 13,638 km under intrinsic forcing from 50 to 200 K.
Stripping gas from 0.1 to 10⁻⁷ Earth masses reduces the selected radius from
17,429 to 9,819 km. The final sub-bar reference (0.110 bar) agrees with bare
rock radius and dimensional inertia within relative 10⁻⁵. Every experiment
checks constituent conservation and reference-gravity closure. These selected
trends are plausibility evidence, not a cooling model, exhaustive continuity
proof or observational accuracy measurement. Existing thin stripping,
water-mass, independent quadrature and tightened-integration regressions remain.

The original validation sample is **140 systems / 1,544 planets**. The expanded
P2 probe is **57 systems per component, 399 systems total**, with the same
deterministic key prefix and full checks on every generated planet. An initial
15,001-system attempt was cancelled without final results; it is not validation
evidence. The probe now logs progress every ten completed systems and flushes
their rows to `data/tmp/composition-coverage-partial.tsv`, so an interrupted run
retains all completed systems (not in-flight work). Final reports are replaced
only after the whole run completes. Checkpoint order is worker completion order,
not a balanced sample if interrupted; the normal final table is sorted by ID.

```sh
NS_COVERAGE_SYSTEMS=57 NS_COVERAGE_THREADS=20 cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture
cargo test --release -p space-simulation source_backed_mass_heating_and_stripping_trends -- --ignored --nocapture
```

The expanded run completed in **82.9 s on 20 threads**: **4,052/4,367 layered
(92.8%)**, plus **315/315 checked labelled fallbacks**, with no missing result.
All record checks passed, and all 1,544 original rows are exactly unchanged in
every non-timing field. The completed-system checkpoint contains all 4,367 final
rows with matching values. This is a deterministic, component-balanced probe,
not a population-weighted statistical accuracy estimate.

Remaining first blockers: cold dense H/He 102, conservative melting screen 53,
outward density inversion 37, envelope work budget 28, warming outside domain
26, H/He-free dense gas 25, central shooting resolution 19, region mass resolution
13, reference-gravity convergence 11 and radial integration convergence 1.
No new failure category appeared. These are fallback reasons, not dropped
planets; no further numerical or physics change was made for this validation.
Evidence is saved locally in `data/tmp/p2-step7-399/`, with run logs at
`data/tmp/p2-step7-{399,trends,envelope,density,check}.log`.

#### Intrinsic temperature census

The figures above and the plausibility audit below describe the original fixed
100 K intrinsic temperature. Retention only sees the fallback radius, so a
layered radius far above it is never checked for escape: `g1724b6d.2.20326/5`
(9.5 Earth masses, 18% H/He, 784 K, 6.9 Gyr) was 30,437 km in the fallback and
87,242 km layered, with the fallback the one consistent with published cooled
models. The mass/age trend was then compared on the same 399-system probe, for
the 717 planets with more than 0.1% H/He. The binding parameter here is
`G M m_H / (k T_eq R)`.

| | Fixed 100 K | Mass/age trend |
| --- | --- | --- |
| Binding parameter below 20 / below 10 | 60 / 10 | 30 / 4 |
| Lowest binding parameter | 4.7 | 7.3 |
| Layered radius over 1.5 / 2 times fallback | 29 / 6 | 1 / 0 |
| Largest layered to fallback radius ratio | 3.38 | 1.70 |
| Layered, all planets | 4,052 (92.8%) | 3,663 (83.9%) |
| Checked labelled fallbacks | 315 | 704 |

The former worst case, `composition-calibration:ThinDisc:5/1`, falls from
90,670 km to 33,175 km. The four planets still below 10 hold at most 4% H/He.
**Layered coverage fell**: cooler interiors leave the supported density domain
more often. First blockers are now cold dense H/He 280, suspended near-critical
151, suspended condensate model 106, conservative melting screen 55 and central
shooting resolution 19. These bodies keep their checked fallback result. An age
exponent of -0.3 was rejected: 13 layered radii exceeded twice the fallback and
one reached 132,000 km at a binding parameter of 4.3. No boil-off rule was
added; escape is still solved on fallback radii only.

#### Saved-population plausibility audit

The post-validation audit scans **all 4,367 saved results**, not just fixtures.
It reconstructs the 315 inexpensive fallback records from the original inputs
and confirms unchanged radii. Layered retained masses account for the saved
additional escape. Density is recomputed from retained mass and selected radius;
`GM_total/R²` is explicitly an **upper bound** on envelope reference gravity,
because the census omits the enclosed reference mass. Four selected layered
outliers are re-solved to inspect complete records; no population re-solve or
physical prescription change is made.

Reproduce after the 399-system census is saved at `data/tmp/p2-step7-399/`:

```sh
cargo test --release -p space-simulation saved_census_fallback_audit -- --ignored --nocapture
PYTHONDONTWRITEBYTECODE=1 python3 scripts/audit-composition-plausibility.py
```

Outputs: `data/tmp/p2-plausibility-{audit.tsv,summary.json,fallbacks.jsonl,outliers.jsonl}`.
The reconstructed fallback's diagnostic reason is an audit placeholder; its
separate `original_refusal` preserves the original census reason.

**Findings, not an all-planets physical validation certificate:**

- Layered radii span 794–90,670 km, bulk densities 13.7–10,067 kg/m³,
  reference inertia factors 0.0156–0.3734 and temperatures 20–1,861 K.
  Fallback radii span 1,740–72,699 km, densities 262–8,270 kg/m³, inertia
  factors 0.0684–0.3710 and temperatures 20–1,136 K. These are broad planetary
  scales, not evidence that each envelope radius is physically accurate.
- **58 distinct layered bodies trigger review**, with overlapping categories:
  20 have density below 100 kg/m³; 28 have a total-mass molecular-H/He binding
  parameter below 20; nine have reference inertia below 0.03; 13 expose rock
  at at least 1,500 K. These are deliberately broad audit triggers, not EOS
  validity limits, new fallback criteria or proven phase/escape diagnoses.
  No body triggers the broad condensed-density (below 500 or above 20,000 kg/m³)
  or inertia (outside 0.01–2/3) screens as implemented in the script.
- **Very puffy envelopes remain a physical concern** (largely resolved since by
  the [mass/age intrinsic temperature](#intrinsic-temperature-census)). The worst is
  `composition-calibration:ThinDisc:5/1`: 7.14 Earth masses, 13.2% H/He,
  90,670 km at one bar and 812 K, density 13.7 kg/m³. Its condensed boundary
  is 10,309 km and its operational 10 mbar shell reaches 158,540 km.
  Full-record mass/boundary checks reproduce, but tighter integration
  (`2e-11`, maximum step `1/256`) exhausts the unchanged work budget. Thus
  **no tightened-refinement pass is claimed for this extreme**, and neither
  the physical radius nor long-term atmospheric retention is established.
- The smallest-inertia case, `composition-calibration:Arms:38#A/1`, has
  3.39 Earth masses, a 44,183 km reference radius and an 8,098 km condensed
  radius. Its small inertia factor is consistent with normalising a compact
  heavy interior by an extended reference radius, not negative/lost mass.
  Tighter integration changes radius/inertia by 5.18×10⁻⁹ / 3.64×10⁻⁸.
  This verifies numerical stability, not the thermal prescription's accuracy.
- The densest case is a 7.53-Earth-mass compressed metal-rich planet with
  radius 10,217 km and density 10,067 kg/m³, plausible for the cold proxy.
  The hottest rock is at 1,861 K and 59.1 bar. Cold refractory mechanics cannot
  determine melting or guarantee a solid landable surface at that temperature;
  this remains an explicit limitation before the P3 consumer/landing cutover.
- All 315 fallback ranges pass the broad physical screens. **279 are labelled
  extrapolated**, using explicitly bounded fit inputs, and 16 also use the
  heavy-mass continuation. All use the documented condensed-secondary proxy.
  Their equilibrium-only temperatures and absent secondary atmospheres are
  known limitations, not greenhouse predictions. No universal accuracy is implied.

The audit therefore supports broad plausibility **with explicit exceptions and
uncertainty**, not the stronger claim that every generated physical value has
been validated. Puffy-envelope thermal/escape plausibility, the extreme's
refinement refusal and hot-rock solidity need acceptance as limitations or
separate follow-up work. No pressure cap, radius rescaling, relaxed work limit
or silent fallback substitution was added to hide them.

The user signed off P2 with these limitations explicitly accepted; future
physics improvements remain targeted follow-up work. No authoritative generation, rendering,
retention or legacy-consumer cutover is included in this validation.

### Checks and coverage

```sh
cargo test --release -p space-simulation pinned_density_and_negative_phase_fixtures -- --ignored --nocapture
cargo test --release -p space-simulation envelope_reference_cases -- --ignored --nocapture
NS_COVERAGE_THREADS=20 cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture
```

Source-backed density/pure-phase fixtures and the paired small-envelope
refinement check pass. Data-free regressions cover thermal consistency with the
decision probe, exact shell-inertia comparison, coupled boundary/origin closure,
protected versus accessible traces, positive water layers, work refusal and
non-landable one-bar transport. Source-backed failures for the cold Jupiter-like
fixture remain recorded; they are not required successes or accuracy references.

The step-3 28 October census solved **1,134/1,544** planets: the previous 1,017 thin
successes plus **117/512 envelope-route bodies** (85 sub-Neptunes, 28 envelope
worlds, four lower-mass gas giants by legacy class). All original non-timing
columns for the 1,017 thin successes are identical to P1. Four new successes
have non-H/He inventory above 20 Earth masses. This is a coupled structural
delivery, **not completed giant coverage**; larger giants remain unsupported.

Step-3 first failures among envelope-route bodies: secondary trace domain
212, shallow water boundary 103, radial integration convergence 25, conservative
melting screen 22, cold dense gas 12, work budget 8, central shooting resolution
7, gravity convergence 5 and region mass resolution 1. The original eight
H/He-free dense-gas and seven warming refusals also remain. These measured gaps
fed P2 step 4; no new water-amount cutoff is permitted to bypass them. The
molecular extension below reclassifies the original trace cohort without adding
generated successes; these counts are the retained step-3 baseline.

### H/He-rich molecular mixtures

`materials::hhe_mixture` adds a uniformly composed molecular envelope model,
`hhe_rich_additive_molecular_volume_v1`. Its first engineering scope requires
**at least 95% H/He by envelope mass**. The separately labelled
[enriched/deep extension](#enriched-and-deep-secondary-envelopes) below now
handles larger secondary fractions; the original constructor and model ID
retain this first scope. The 5% secondary bound is a conservative
scope restriction, not a measured phase threshold or a claim that every
H/He-dominated envelope is now supported. CO₂/N₂-dominated atmospheres remain
outside this initial delivery. The extension is an approximate structure route,
not resolved metallic-depth impurity thermodynamics.

For each present secondary, `materials::molecular` supplies a pure-component
Peng–Robinson gas/supercritical density at the **same total pressure and
temperature** as the screened H/He endpoint. The specific volumes are combined:

`v_mix(P,T) = (X_H + X_He) / rho_HHe(P,T,Y_He) + sum_i X_i / rho_i(P,T)`

`rho_mix = 1 / v_mix`, with `Y_He = X_He / (X_H + X_He)`.

H/He interactions remain included once through CMS19/HG23; no second H/He
mixing correction is applied. Cross interactions with secondaries and excess
mixing volumes are neglected. This is an approximate structural mixture, not a
validated binary-fluid EOS or an equilibrium flash. Fractions remain uniform
within the envelope, while pressure, temperature and density vary radially.
Temperature still follows the prescribed H/He-dominated profile; composition
does not update opacity, heat capacity or the fixed deep gradient in this slice.

All positive supported constituents are evaluated and retain separate origin
inventories. Protected material is never transferred out of the heavy interior;
water never joins the mixed parcel, and ammonia is not introduced. No molecule
is silently removed because its abundance is small. If a present component is
unsupported, the new route refuses the whole structure.

#### Component equation and restrictions

The molecular component EOS is the original Peng–Robinson form:

`P = R T / (v-b) - a alpha / (v² + 2 b v - b²)`

`a = 0.45724 R² Tc² / Pc`, `b = 0.07780 R Tc / Pc`,
`alpha = [1 + kappa (1 - sqrt(T/Tc))]²`,
`kappa = 0.37464 + 1.54226 omega - 0.26992 omega²`.

Here `v` is molar volume. Critical parameters and acentric factors for CO₂,
N₂, CH₄, O₂ and Ar are rounded from the pinned CoolProp v7.2.0 cubic catalogue;
the pressure equation and cubic-root solve are implemented locally, with no
CoolProp dependency or new runtime resource. Provenance and independent NIST
checks are in the [reference guide](planet-composition-reference.md#bounded-molecular-component-density).

The **application** rectangle is 80–625 K and **0.01 mbar to 1,000 bar**. It is
not an assertion of uniformly accurate density throughout that rectangle. In
particular, cubic fluid equations cannot establish solid phases, high-temperature
chemical survival or planetary metallic-pressure behaviour. This bounded
molecular model must not be extrapolated to those states.

Additional conservative screens:

- Below a component's critical temperature, total pressure must remain below
  the existing rough pure-component vapour/frost cap. Subcritical oxygen is
  refused because that shared cap supplies no oxygen constraint. This is a
  conservative pure-endpoint restriction, **not** a mixture dew/frost point;
  dilution can keep real mixtures gaseous where this model refuses them.
- The largest physical cubic root must have positive compressibility. Its
  polynomial residual must close within relative scale 10⁻¹². Below critical
  temperature, a compressed-liquid single root is refused; competing physical
  roots are checked by their cubic fugacity coefficients, rather than silently
  returning metastable vapour. No liquid secondary layer is created.
- Exclude the near-critical box `abs(T/Tc - 1) < 0.1` with `P > 0.5 Pc`, where
  cubic densities are particularly sensitive. This exclusion is an engineering
  mask, not a phase boundary.
- All existing H/He quantum, melting, cold-density and interpolation masks,
  heavy-water bounds, gravity closure and numerical budgets remain unchanged.

The old passive-trace envelope model stays unchanged through 0.301% secondary
mass, preserving all previous envelope successes exactly. Above it, the new
explicit molecular model is selected; failures never fall back to the passive
model. This is an explicit approximation/domain boundary, not a physical phase
transition. Overlap fixtures compare the two density prescriptions at that
boundary, with differences below 0.31% in the sampled states. The molecular
closure itself varies continuously with enrichment and approaches the exact
H/He endpoint as secondary abundance vanishes. These overlap checks are not a
whole-domain or planet-radius accuracy claim.

#### Checks and generated coverage

Tests compare ten sparse gas/supercritical density points against the independent
NIST WebBook reference (within 4% for the component approximation), verify the
analytic dilute molecular-weight limit, pressure compressibility, enrichment
continuity, positive-species handling, unsupported states and condensation
refusal. Source-backed coupled tests use a one-Earth-mass dry heavy body,
`1e-5` Earth masses of H/He and 2% secondary envelope mass. Its one-bar radius is
about **6,499 km**, with an interface near **12.30 bar**; radius and inertia
refine within relative 10⁻⁷. The pure-density approximation tolerance does not
relax the radial solver's relative 10⁻¹⁰ integration tolerance.

```sh
cargo test --release -p space-simulation materials::
cargo test --release -p space-simulation rich_molecular -- --include-ignored --nocapture
NS_COVERAGE_THREADS=20 cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture
```

At initial mixture delivery, adding positive water returned a water-load refusal:
the molecular pressure ceiling is 1,000 bar, whereas the deep loaded-water
prescription required its top at 10,000 bar or above. All generated candidates
retain positive water. The subsequent shallow-water slice below closes that
particular domain mismatch without discarding water or widening molecular bounds.

The mixture-only 20-thread rerun solved **1,134/1,544**, with every original
non-timing coverage column identical for all 1,134 previous successes. Current
outcomes of the original 212-body trace cohort:

| Current outcome | Bodies |
| --- | ---: |
| Outside the first 5% secondary composition bound | 136 |
| Conservative pure-component condensation screen | 42 |
| Molecular pressure/temperature domain | 25 |
| Existing shallow-water boundary | 8 |
| Radial integration convergence | 1 |

The 76 inside the new composition bound have progressed beyond the old trace
guard, but **none is a new solved generated planet**. Domain/phase errors may
arise during rejected shooting trials; they are not measured deep conditions of
an accepted planet. The original cohort stays in the diagnostic TSV with its
current outcome so that reclassification cannot masquerade as coverage gain.
Full project checks and the source-backed mixed-envelope refinement test pass.

### Shallow loaded-water approximation

`water::loaded`, model `shallow_loaded_water_murnaghan_proxy_v1`, replaces the
blanket refusal for a separate water parcel whose envelope load is below
**10,000 bar**. Its pressure-only engineering Murnaghan law is:

`rho(P) = 1000 [1 + 7 P / (2.2e9)]^(1/7)` (SI).

The ambient liquid-like density (1,000 kg/m³), bulk modulus (2.2 GPa) and pressure
derivative (7) are plausible engineering assumptions, **not a fitted published
thermal EOS**. The application pressure range is 0–10,000 bar, with the envelope
solver retaining its existing 10 mbar outer pressure. A room-temperature overlap
check differs from the existing IF97 liquid density by less than 1% at 1, 100 and
1,000 bar; this is not a physical-error bound away from those three points.

Temperature is deliberately absent from the compression law. The parcel remains
unresolved water, even where actual water could freeze, become steam, mix into
H/He or react at depth. It is especially uncertain for hot, weakly loaded water;
it does not establish a stable condensed boundary or a thermal/phase solution.
The displayed temperature is the envelope prescription at the region's top,
not a solution for heat transport inside the water. No ocean, solidity or landing
permission follows from this result. The label is `approximate`, with the above
limitations recorded per layer, not `reference_checked`.

Where the water interval crosses 10,000 bar, the existing kernel locates a model
interface with separate one-sided densities and disjoint, conserved water
allocations. The deep Seager density is about 1,589 kg/m³ and the shallow proxy
about 1,227 kg/m³ there. This intentional, unblended model discontinuity is not
a phase boundary or matched thermodynamic equilibrium. No step crosses it using
one EOS and no density offset conceals it. Water that remains entirely deeper
uses the previous compression law unchanged; different rejected shooting trials
can change work and roundoff in the accepted profiles.

The slice-1 20-thread census solved **1,232/1,544 (79.8%)**, up **98**, with no lost
successes, including eight from the original molecular-mixture cohort. Of 111
former shallow-water refusals, 95 solve; the other 16 expose
radial convergence (9), cold dense H/He (5), pure melting screen (1), or missing
one-bar event (1). Three previous radial failures also solve after the changed
trial paths. All previous successes remain numerically stable: maximum relative
radius change is 1.36×10⁻⁹ and inertia-factor change 2.72×10⁻⁹. This does not
close the remaining composition, phase, dense-H/He or numerical gaps.

Headless fixtures check constituent/protected allocation closure, located model
interfaces, independent radial water-mass/hydrostatic quadrature, transport
labels and fourfold-step/fivefold-tolerance refinement. Source-backed tests use
the existing local H/He resource; standalone material checks need no resource:

```sh
cargo test --release -p space-simulation materials::water::loaded
cargo test --release -p space-simulation shallow_loaded -- --include-ignored --nocapture
```

### Enriched and deep secondary envelopes

The next step-4 slice adds an explicit enriched constructor and a separate
hot/deep component approximation, without changing the standalone molecular EOS
mask or its source-checked results. The original 5% constructor still refuses
larger fractions. Runtime envelopes above the passive 0.301% trace allowance now
use the enriched route, with **positive H/He still required**. Secondary-dominated
envelopes are allowed, not just H/He-rich ones; H/He-free atmospheres remain a
separate gap. Protected inventory and all positive water remain separate heavy
parcels. Using water-like density never converts CO₂/N₂/CH₄/O₂/Ar into H₂O.

#### Explicit component handoff

The radial solver retains screened Peng–Robinson secondary densities wherever
the prescribed thermal profile is within the existing molecular mask. Going
inwards, the component boundary is the earlier of **1,000 bar** or **625 K**.
Its pressure is recomputed for each gravity iteration by bounded bisection of the
same monotonic thermal prescription. If the entire envelope is already hotter
than 625 K, it uses the effective model throughout, with no molecular anchor.
The 80 K lower molecular bound remains unchanged.

`effective_secondary`, model `thermal_plus_water_compression_secondary_proxy_v1`,
uses a simple specific-volume sum for each secondary component:

`rho_water_proxy(P) = 1460 + 0.00311 P^0.513`

`rho_raw_i(P,T) = 1 / [R_i T/P + 1/rho_water_proxy(P)]`

`R_i = k_B / (m_u mu_i)` (all equations use SI).

The first term gives the dilute molecular-weight limit; the second is the
existing Seager **water** compression row reused as an explicitly uncertain
heavy-material surrogate. It is not a published EOS for nitrogen, carbon dioxide
or their metallic/dissociated mixtures. No molecular survival, chemistry or phase
is inferred at depth. The effective application scope is **80–100,000 K** and
**0.01 mbar–100,000,000 bar**, intersected with all unchanged H/He screens,
thermal and full-planet numerical limits. The upper bounds are engineering stops,
not source-validated molecular-fluid domains.

Where a molecular endpoint exists, each present component has an explicit
continuity normalisation:

`s_i = rho_PR_i(P_join,T_join) / rho_raw_i(P_join,T_join)`

`rho_effective_i(P,T) = s_i rho_raw_i(P,T)`.

This makes the handoff continuous in density, not thermodynamically matched.
The factors are fixed within one gravity iteration and recomputed with the
thermal boundary; derivatives, entropy and Gibbs potentials are not matched.
This declared anchoring is part of the approximation, not a hidden density offset
or empirical fit. An entirely hot envelope has `s_i = 1`, because there is no
in-domain PR endpoint. Its onset at the outer thermal boundary is not claimed
continuous as an evolution model. Within a joined profile the effective volume
decreases inwards for the prescribed gradient, so density is compressive and no
artificial outward density inversion is introduced at the handoff.

The kernel locates the component event separately from the exact one-bar event,
with disjoint conserved region allocations and one-sided density evaluation.
Coincident events are represented only once. Anchor evaluations are lazy, cached
per gravity iteration and charged to the same EOS work budget, including failures;
an inaccessible deep state does not reject a shallow successful profile. No
failed PR call is caught and turned into an effective-fluid success. Route
selection uses the explicit thermal/pressure boundary, and both sides retain
conservative pure-condensation and near-critical screens. The full PR vapour
root/stability checks remain on the molecular side. No liquid or solid secondary
regions are introduced in this slice. The later [suspended-condensate route](#suspended-secondary-condensates)
explicitly replaces the cold mixed-envelope closure, not this pure API.

Total density still uses additive specific volumes at common total P–T, with
CMS19/HG23 H/He evaluated at its internal helium fraction. Cross interactions,
excess mixing volume, composition-dependent opacity/heat capacity, dissociation,
ionisation and phase separation remain unresolved. In particular, the original
H/He-oriented grey profile and fixed **2/7** deep slope are kept even when
secondaries dominate. This is a potentially important physical uncertainty,
not a scientifically validated heavy-atmosphere thermal model.

#### Transport, checks and measured gain

Shallow enriched regions use `secondary_enriched_additive_molecular_volume_v1`;
effective regions use `additive_effective_secondary_volume_v1`. A whole envelope
that uses the hot/deep route reports
`molecular_to_effective_secondary_envelope_v1`. Existing shallow H/He-rich
successes retain their original IDs. Every extended region remains `approximate`,
with its compression/thermal/chemistry limitations visible in transport and the
inspector. Neither the proxy's pressure reach nor tight numerical refinement
establishes whole-planet physical accuracy.

Data-free tests cover enriched ideal molecular weight, positive constituents,
compression along the prescribed gradient, refusal outside application bounds,
phase/critical screens, matched one-sided densities, event ordering on either
side of one bar, cache accounting and every insufficient anchor budget. Coupled
20% and 90%-secondary fixtures use a five-Earth-mass refractory host, 0.1 Earth
masses of H/He, 0.03 Earth masses of separate water and protected CO₂. Independent
radial mass/inertia/hydrostatic quadrature and refinement check numerical closure,
not the physical accuracy of the surrogate.

With the local source-backed H/He resource, their radii are about **16,276 km**
and **13,369 km**, with component handoffs near **23.29 bar** and **30.67 bar**.
Radius and integrated inertia refine within relative 10⁻⁷. The new density
surrogate requires no additional runtime data or library.

The slice-2 20-thread census solved **1,304/1,544 (84.5%)**, up **72**, with no lost
successes. Every prior non-work coverage column, including reported radii and
inertia factors, is exactly unchanged. Full project checks and all 14 envelope
tests (including source-backed fixtures) pass.
Of the 136 composition refusals, 50 solve, 62 reach the condensation screen,
23 cannot resolve a one-bar event, and one fails radial convergence. Of the 25
molecular state refusals, 22 solve, two reach condensation and one fails radial
convergence. A missing one-bar reference is still a refusal, not permission to
invent ground or return an incomplete structure.

```sh
cargo test --release -p space-simulation effective -- --nocapture
cargo test --release -p space-simulation enriched -- --include-ignored --nocapture
NS_COVERAGE_THREADS=20 cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture
```

### Suspended secondary condensates

`materials::suspended` introduces `local_suspended_secondary_saturation_v1` for
cold mixed envelopes. It is a **perfect-entrainment approximation**: total
constituent abundances stay uniform, while vapour and condensate fractions vary
locally with pressure and prescribed temperature. Condensate occupies volume,
contributes to gravity/inertia and stays in its local parcel. It does not rain
out, settle into the water layer or gain protection. This is not a cloud-weather
simulation, non-ideal equilibrium flash or resolved liquid/solid phase diagram.

#### Audit and deterministic selection

Before implementation, all **106** condensation-screen failures involved CO₂.
Test-only instrumentation recorded the last observed refused trial state per
body: **84.17–189.52 K**, **10–664.58 mbar**. Using the old saturation cap with
the uncondensed mixture's CO₂ partial pressure reclassifies 40 sampled states as
undersaturated and 66 as supersaturated. These are diagnostic estimates at
rejected shooting trials, not accepted cloud layers or confirmed condensates.
The raw pre-change audit is retained in the local slice results directory.

The new route is selected **before shooting**, whenever a present secondary's
critical temperature exceeds the thermal prescription's minimum
`(T_eq^4 + 0.5 T_int^4)^(1/4)`. This minimum is independent of iterated gravity;
the route cannot switch on a failed EOS call or between gravity iterations.
Warm explicit mixtures retain the previous PR/effective closure. The original
passive trace route through 0.301% secondary mass is unchanged. Selecting the
cold route does not assert actual condensation. All existing H/He, water,
one-bar, conservation and work restrictions remain.

#### Local partition and density

For each secondary, let `a_i = w_i / mu_i` be its available molecular abundance
per unit total parcel mass and `K_i = P_sat_i(T)/P`. For the noncondensing H/He
let `n_light = w_H/mu_H + w_He/mu_He`. Solve

`n_i_vapour = min(a_i, K_i N)`

`N = n_light + sum_i n_i_vapour`.

A monotonic active-set solve caps saturated constituents simultaneously and
removes them from the free molecular pool. At most five removals and one closure
pass are needed. Saturated species have `y_i P = P_sat_i`, and all others remain
undersaturated; vapour plus condensate retains each origin. Calculating separate
ratios against the original, undepleted gas would not satisfy this shared mole
balance. Negative allocations, lost positive fractions or unresolved mole
closure remain errors.

The phase estimate uses **ideal partial pressures**: vapour fugacity,
condensate activity and Poynting correction are all set to one. This is an explicit
approximation even in dense mixtures, not a validated non-ideal correction. It
is most credible for dilute vapour and separate nearly pure condensates. Dense
mixture phases, dissolved gases and critical phenomena remain uncertain.

Separate continuous Clausius–Clapeyron branches run through approximate triple
and critical anchors. Below each triple temperature use the listed sublimation
slope; above it use the slope fixed by its triple and critical pressures:

`ln(P_sat/P_triple) = -L_over_R (1/T - 1/T_triple)`

`L_over_R_liquid = ln(P_critical/P_triple) / (1/T_triple - 1/T_critical)`.

| Constituent | Triple anchor, K | Triple pressure, bar | Sublimation slope, K | Condensate reference density, kg/m³ |
| --- | ---: | ---: | ---: | ---: |
| CO₂ | 216.58 | 5.185 | 3135 | 1560 |
| N₂ | 63.14 | 0.12520 | 850 | 808 |
| CH₄ | 90.694 | 0.11696 | 1160 | 450 |
| O₂ | 54.361 | 0.001463 | 1000 | 1140 |
| Ar | 83.806 | 0.68891 | 930 | 1400 |

Critical constants retain the bounded molecular catalogue's values. No
single triple-to-critical slope meets the oxygen spot check closely enough, so
O₂ additionally joins through **90.188 K / 1.01325 bar**, using a separate
Clausius–Clapeyron slope on each side of that boiling anchor. Pressure remains
continuous at every anchor; no condensed-density phase jump is introduced. No
condensation is assigned at/above a constituent's critical temperature. The
near-critical exclusion `abs(T/Tc - 1) < 0.1` at `P > 0.5 Pc` is retained for
every present secondary. This prevents a discontinuous high-pressure critical
phase assignment from becoming a successful structure. Triple anchors only
join the saturation branches: no liquid/solid identity, melting curve or density
jump is inferred there. The existing legacy cold-trap curves are unchanged.

Within **80–625 K** and **0.01 mbar–1,000 bar**, secondary vapour uses ideal
thermal volume; condensate uses a separate Murnaghan pressure-only proxy:

`rho_cond_i(P) = rho0_i [1 + 7 P/(2e9)]^(1/7)` (SI).

The reference densities are engineering liquid/solid-like values; the shared
bulk modulus **20,000 bar** and pressure derivative 7 are not constituent EOS
fits. Thermal expansion and dense solid/liquid differences are unresolved.
The total specific volume is

`v = w_light/rho_HHe + sum_i [w_i_vapour R_i T/P + w_i_cond/rho_cond_i(P)]`.

H/He still uses the screened CMS19/HG23 density at total P–T and its internal
helium fraction. Using each secondary's partial pressure directly as its
additive-volume density would double-count dilution; the formula above instead
has the correct ideal-mixture molecular-weight limit. Both phase amounts are
normalised to **total parcel mass**, not just remaining vapour.

At the first inward 625 K or 1,000-bar model boundary, the deep effective
secondary proxy is normalised to this route's **ideal-vapour** endpoint, not PR.
A successful deep handoff requires every present secondary to be at/above its
critical temperature there; otherwise the route refuses. The handoff density is
continuous and separately event-located, with the unchanged exact one-bar event.
Continuous saturation-onset kinks use the existing adaptive integration, not a
density jump or an unlocated mass transfer. This piecewise closure is not a
continuous planetary evolution model across the warm/cold route-selection bound.

The local partition is evaluated inside every radial density call. Each gravity
iteration therefore recomputes phases and density along its new thermal profile;
no outer iteration moves mass between condensate and an invented reservoir.
The thermal prescription still ignores latent heat, condensate opacity and
composition-dependent heat capacities, retaining its **2/7** deep slope. A
profile with an outward density inversion still refuses, explicitly labelled
`outward envelope density inversion`, rather than blamed on H/He interpolation.
Such failures can indicate incompatibility of perfect suspension with the
prescribed thermal structure, not permission to remove the stability check.

#### Transport and checks

`Layer.phase_allocations` reports vapour/fluid and locally suspended condensate
kilograms per constituent. These are **suballocations** of each layer's existing
mass, never extra layers or extra gravity. A mass-coordinate profile trapezoid
estimates the integrated phase amounts; weighted averages remain bounded by
each available constituent. Vapour is the conserved remainder. The transport
material budget counts suspended mass under `condensed_kg`, not `gas_kg`, but
this does not make `condensed_radius_m` the boundary of every condensate or
grant landing permission. Protected material and separate water stay unchanged.

Each cold-route region is labelled `approximate`, with suspension, phase,
density and thermal limitations visible in the inspector alongside its phase
split. Deep regions retain the explicit effective-volume model ID; the whole
joined envelope uses `suspended_to_effective_secondary_volume_v1`. Standard
checks need no new data or dependency. Pure PR masks and NIST regressions remain
unchanged, not silently widened to support this route.

Tests cover simultaneous condensation, dilution false alarms, saturation onset,
evaporation on warming, ideal molecular weight, independent sparse NIST checks,
invalid/near-critical states and positive compression. Coupled fixtures include
separate water and protected CO₂ on a one-Earth-mass refractory host with 0.01
Earth masses of H/He and 2%/20%-secondary envelopes at 100 K equilibrium and
intrinsic forcing. Source-backed one-bar radii are about **11,170/10,398 km**.
They pass independent envelope mass/hydrostatic quadrature and relative 10⁻⁷
radius/inertia refinement. Integrated condensate fractions refine within
relative 10⁻³ (observed below 0.012%), a separate reporting-quadrature check,
**not a relaxation of radial integration or constituent closure**. Tight
numerical refinement does not validate perfect suspension or the ideal phase law.
An explicit secondary-dominated inversion fixture remains a required refusal.

```sh
cargo test --release -p space-simulation suspended -- --include-ignored --nocapture
NS_COVERAGE_THREADS=20 cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture
```

At the suspended-condensation checkpoint, the 20-thread census solves **1,380/1,544 (89.4%)**, gaining **76**
from the original 106 condensation refusals, with no previous successes lost.
Of the 76 new solves, 51 have integrated CO₂ condensate and 25 have none anywhere
in their accepted profile. Across all 125 accepted cold-route planets, 51
contain condensate, all CO₂; the rest use the mixture-aware vapour law. The
remaining original candidates refuse on cold dense H/He (15), outward density
inversion (12), radial convergence (2) or central shooting resolution (1).
Among previous successes, 49 cold explicit mixtures intentionally change their
density prescription: maximum relative radius change is **0.01144%**, and
inertia-factor change **0.02287%**. These are model differences, not physical
error bars. The other previous successes retain their reported physical results.
That checkpoint leaves 164 layered refusals; subsequent numerical recovery is
described above. The current target is about 95% layered with a labelled fallback
for the rest, not 99%. Coverage diagnostics also write `composition-condensation.tsv` (observed
pure-screen trial states) and `composition-suspended.tsv` (accepted cold-route
phase totals), without altering runtime generation.
Full project checks, all 17 envelope tests (including source-backed cases) and
all seven focused suspended-model tests pass. At that historical checkpoint,
rendering remained legacy; no
interactive benchmark or visual test was performed.

### Secondary trace input investigation

The coverage probe also writes `data/tmp/composition-traces.{md,tsv}` for the
original trace-guard cohort, including per-material protected/accessible inventories,
both host-relative fractions, the prior thin-route refusal and current outcomes.
Selection remains based on the old guard, so reclassified refusals remain in
the investigation. This is test-only
instrumentation: it neither changes generation nor bypasses any solver guard.
The protection floor is reconstructed from the source inventory, not by taking
90% of the post-escape retained inventory. Synthetic tests distinguish both
guards, simultaneous failures and accessible loss with unchanged protection.

The 28 October investigation found **all 212 fail the accessible-envelope
fraction**, not the protected-interior fraction. Protected fractions reach only
0.2667% of heavy mass, below the 0.301% bound. Their prior thin-route refusals
are 203 dense-gas, eight gas-mass and one heavy-mass refusal. There are no giant
classes here: 101 rocky, 39 metal-rich, 34 water-rich and 38 sub-Neptunes by
legacy labels. Masses span 0.102–23.13 Earth masses.

| Accessible secondary mass fraction of envelope parcel | Bodies |
| --- | ---: |
| 0.301% to below 1% | 33 |
| 1% to below 5% | 43 |
| 5% to below 10% | 16 |
| 10% to below 50% | 41 |
| At least 50% | 79 |

The median is 15.04%, reaching nearly 100%. These are **parcel fractions**, not
planet mass fractions: median total envelope mass is only 0.01083% of the planet,
and 174/212 have H/He below 0.1% of planetary mass. All retained secondaries
combined remain below 0.296% of total planet mass in this sample. Small residual
H/He supplies make otherwise small secondary inventories non-trace components
of that outer parcel. CO₂ dominates the accessible secondary mass in 95 bodies,
nitrogen in 117; both occur in all 212. Methane and argon each contribute at most
1% of the envelope; oxygen is absent. Ammonia is not a generated constituent.

This changes the practical requirement: a trace extension for H/He-rich
envelopes alone cannot represent the secondary-dominated end. Dense mixed
secondary atmospheres with residual H/He need their own justified mechanical
prescription, condensation handling and thermal bounds. Uniform composition
within a gas/fluid region is a possible simplification, not permission to use
H/He density for substantial CO₂/N₂ or to mix protected/water inventories into
the atmosphere. The investigation itself selected no mixture prescription; the
separate bounded delivery above now attempts the first 76 candidates, without
claiming complete coverage of their conditions.

Irradiation equilibrium spans 1.51–646.05 K (median 184.65 K), but is **not**
the actual envelope temperature; runtime irradiation flooring and intrinsic
heat still apply. The original trace refusals happened before envelope integration, so no
accepted interface pressure, deep temperature or metallic-hydrogen exposure
exists for this population. Those cannot be inferred from the legacy atmosphere
or by suppressing the trace guard. Further state-range investigation needs an
explicitly bounded candidate model and must not count provisional profiles as
successful planets. The 212 were first blockers, not guaranteed future wins;
the bounded molecular delivery confirms this distinction.

## Conserved inventory, not a classification

`Material` has stable string identifiers. Adding a constituent requires an
explicit catalogue entry and physical prescriptions, not just a density:

| Identifier | Meaning / supported prescription |
| --- | --- |
| `iron_nickel` | Unresolved metal; cold pure-Fe radius proxy |
| `silicate` | Unresolved silicate rock; cold MgSiO₃ proxy |
| `water` | H₂O mass, independent of reservoir/phase; cold water-ice radius proxy |
| `hydrogen`, `helium` | Separate H₂ and He mass budgets; joint light envelope model |
| `nitrogen`, `carbon_dioxide`, `methane`, `oxygen`, `argon` | N₂, CO₂, CH₄, O₂ and Ar molecular budgets |

Material identity is centralised, but not all physical-property ownership has
migrated: legacy fit/gas mappings remain in `composition/catalogue.rs`, molecular
masses in `atmosphere.rs`, and approximate cold-trap curves in
`atmosphere/condensation.rs`. Cold refractory, water and optional H/He density
physics live under `materials`. Methane is already drawn by formation and can enter the gas
budget; neither methane liquids/lakes nor condensed CO₂/supercritical CO₂ are
resolved by the new radial/coupled path. A cold-trap cap is not a phase diagram.
The cold mixed-envelope route above now assigns approximate suspended secondary
condensates, not separate resolved liquid/solid surfaces. Elsewhere methane is
treated as an ordinary gas; dedicated methane phase work is parked on
the `park/methane-physics` branch.
The [replacement contract](../plans/planet-composition.md#replacement-contract)
describes the remaining consolidation and extensions, not current behaviour.

These are material/molecular budgets, **not independent elemental inventories**.
The hydrogen bound in water or methane is included in that molecule's mass,
not counted again in `hydrogen`. Likewise oxygen in rock, water and CO₂ is not
also placed in `oxygen`. Metal alloys and mineral species are unresolved.
Oxygen support does not imply biological production or generated free oxygen.
Reactions, dissociation and conversions between species are unsupported: future
chemistry must explicitly conserve atoms and debit all source molecules.

`Inventory` accepts finite nonnegative kilograms, rejects duplicate constituents
and nonfinite totals, and keeps masses private. Empty reservoirs are valid; an
empty planet is not. Mass-fraction construction requires a positive total and
fractions summing to one within 1e-9, normalising only that rounding error.
The empty inventory has unknown fractions (`None`), not a fictitious composition.

`Budget` holds the formation/source inventory, retained inventory, three retained
reservoirs and escaped inventory. The reservoirs are interior, unresolved
condensed volatiles and the **entire** gaseous/fluid envelope. Source mass minus
allocations becomes escaped mass, separately for every constituent. Overspending
even a trace constituent is rejected without an absolute tolerance. No reservoir
assignment asserts equilibrium chemistry or a resolved ocean. Atmospheric columns
are descriptions of gas-reservoir material, never additional inventory.

Transfers between retained reservoirs are atomic on failure and do not change
source, retained or escaped totals. They conserve allocations to floating-point
roundoff, not arbitrary precision. A positive transfer too small to change either
source or destination mass is rejected rather than manufacturing a trace supply.
Escape is not silently recaptured. A later evolution step can construct a new
budget with revised retained allocations and explicit escaped remainder.

Gas conversion uses the atmosphere catalogue's existing molecular masses:
`w_i = x_i μ_i / Σ(x_j μ_j)` and `x_i = (w_i/μ_i) / Σ(w_j/μ_j)`.
Here `w` is mass fraction and `x` is mole fraction. Conversion preserves molecular
identity and total mass; solid rock/metal cannot be converted to gas by this API.
Condensation must explicitly transfer the same molecular material to condensates.

## Cold compressed heavy-material radius

This section describes the current **generation** model, not the separate radial
solver below.

Pure-material radii use Seager et al. (2007), equation 23 and the **modified-
polytropic** rows of table 4, not the separate unparameterised full-EOS rows:

```text
s = M / m1
R / r1 = 10^[k1 + log10(s)/3 - k2 s^k3]
```

| Proxy | m1 (Earth masses) | r1 (Earth radii) | k1 | k2 | k3 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fe | 5.80 | 2.52 | -0.209490 | 0.0804 | 0.394 |
| MgSiO₃ | 10.55 | 3.90 | -0.209594 | 0.0799 | 0.413 |
| H₂O ice | 5.52 | 4.43 | -0.209396 | 0.0807 | 0.375 |

We restrict direct fits to heavy-material mass ≤20 Earth masses. From 20 to
1,000 Earth masses, a continuous `R(20) (M/20)^0.2` continuation is explicitly
flagged. This is our bounded engineering extension, not the paper's EOS solution.
Below 0.0001 or above 1,000 heavy-material Earth masses the model returns an error;
it does not extrapolate indefinitely into asteroids or massive solid giants.

Mixtures use `R³ = Σ w_i R_i(M_heavy)³`, evaluating every pure curve at the **same
total heavy mass**, not each constituent's separate mass. This is compressed
volume interpolation, not addition of ambient-density volumes and not a layered
hydrostatic interior integration. Mixtures are flagged. Secondary molecular
volatiles have no pure condensed fit; only a combined ≤1% of heavy mass may use
an explicitly flagged water proxy. Larger supplies return an unsupported-domain
error rather than silently treating arbitrary bulk material as water.

Water-bearing results are flagged as cold-water proxies. Their radius is not
evidence of liquid water, a particular ice phase, an exposed ocean or solid ground.
An H/He planet's heavy-material radius starts as a cold structural proxy, not a
located core/envelope discontinuity. The bounded environmental correction below
can promote it to an approximate thin-atmosphere boundary. Dissolved H₂/He is
currently unsupported.

## Headless dry-body solver

`system::structure::dry` is an opt-in, spherical radial solver, not the generation
default. The immutable request is a retained `Inventory`, imposed outer pressure
and bounded numerical controls. The fixed model `cold_differentiated_seager_2007_v1`
places all metal inside all silicate and omits zero-mass regions. Disjoint region
inventories own allocation; their sum derives retained inventory. No mutable
reservoir copy or legacy radius fallback is used along this path.

Shared material identities now live in `system::materials`, re-exported from
`composition` for existing callers. `materials::cold` supplies Seager et al.
(2007), equation 11/table 3: `ρ(P)=ρ₀+cPⁿ`, in SI units:

| Proxy | ρ₀ (kg/m³) | c (kg/m³ · Pa⁻ⁿ) | n |
| --- | ---: | ---: | ---: |
| Fe | 8300 | 0.00349 | 0.528 |
| MgSiO₃ | 4100 | 0.00161 | 0.541 |

The input mass domain is 0.0001–20 Earth masses and the pressure domain is
0–10¹³ Pa **throughout**. These constraints intersect: not every mixture/load
in the mass range can be solved below that pressure ceiling. The tested
20-Earth-mass, 70%-metal and nearly/pure-metal fixtures return an explicit
unbracketed error at the ceiling. Any positive water or gas, even a trace, is
unsupported. An imposed external pressure allocates no atmospheric mass.
Temperature and phase remain unresolved, including core solidity. Thermal
expansion, reactions, porosity, mixtures within a region and rotational structure
are not modelled. These are cold material proxies, not a reconstruction of Earth.

The solver integrates mass conservation, hydrostatic balance and axial inertia.
It uses dimensionless volume and `x=(m/M)^(1/3)` as the independent coordinate,
with a regular centre expansion, adaptive Dormand-Prince 5(4) integration and
central-pressure bracketing. The boundary residual exists only above the
root, so shooting takes a secant through the two latest high-side trials,
aimed at the middle of the accepted residual band, and bisects after any
undershoot or doubtful slope. The envelope route retries a boundary-resolution
failure by plain bisection, with its own per-solve evaluation ceiling. Steps end exactly at material mass
coordinates; interface radius, mass and pressure are continuous, while each
region retains its own one-sided density. Error control measures volume/inertia
**increments**, rather than letting a large accumulated state hide error in a
trace shell. Negative-pressure trial stages are retried at smaller steps, never
clamped or fed into an extrapolated EOS. No final radius rescaling or pressure
translation repairs a failed solve.

Results expose ordered profiles, material boundary radius, gravity, density,
integrated inertia, model identity and raw pressure/work diagnostics. Profiles
are accepted only with finite in-domain ordered states and per-region mass
resolution to relative 10⁻⁸. If a tiny region cannot be represented by distinct
mass/radius endpoints, it fails explicitly instead of losing its material.
Allocation closure is species-local to relative 10⁻¹⁰ (exact for this pure-region
allocation). The outer pressure residual is at most `max(1 Pa, 10⁻⁸ P_c)`, from
above; this dry-body global criterion does **not** certify surface phase physics.
Reference-pressure level lookup, atmosphere matching and transport are not yet
part of this API.

Numerical ceilings are 64 bracket expansions, 128 shooting iterations, 32,768
attempted integration steps per trial and 2,000,000 EOS evaluations per request.
Rejected steps and output density evaluations count towards those budgets. Controls
may reduce ceilings, not raise them. Unsupported inputs, exhausted budgets,
unbracketed roots, unresolved numerical intervals and invalid closure are typed
failures, never partial successful planets or invitations to substitute a fit.

Reproduce headless validation and the diagnostic reference table:

```sh
cargo test -p space-simulation system::structure::dry
cargo test -p space-simulation prescribed_dry_reference_profiles -- --nocapture
```

Tests cover uniform and two-layer constant-density analytic solutions to relative
10⁻⁶, independent radial quadrature of EOS density/inertia to relative 10⁻⁸,
hydrostatic pressure-drop quadrature to relative 2×10⁻⁴, interface continuity,
pure endpoints and 10⁻⁶-mass-fraction trace core/mantle fixtures. The grid spans
0.0001, 0.0553, 1, 5 and 20 Earth masses, with ceiling failures asserted rather
than replaced. Tightening integration tolerance by four and the maximum step by
four changes accepted radius, inertia factor and central pressure by less than
relative 10⁻⁵ on the refinement fixtures. Radial quadrature uses independent
Gaussian integration of endpoint-Hermite pressure profiles, not cached mass
coordinates or the solver's integrated inertia. Tests also check deterministic
repeats, loading/compression and each work-budget failure. These tolerances
measure numerical consistency, not scientific error bars.

| Prescribed input | Radius (km) | Central pressure (bar) | I/(MR²) |
| --- | ---: | ---: | ---: |
| Mercury-like, 0.0553 Earth masses, 70% metal | 2248.354 | 500871.310 | 0.34434286 |
| Earth-like, 1 Earth mass, 32.5% metal | 6113.591 | 3908596.767 | 0.34026229 |
| Super-Earth, 5 Earth masses, 32.5% metal | 9764.913 | 17179412.883 | 0.32777865 |

These diagnostic fixtures use vacuum outer pressure and are not exact matches to
the named Solar System bodies. No coefficients were tuned to match them.

## Headless protected traces in dry hosts

`system::structure::dry::protected::solve(&host, &protected, outer_pressure_pa,
Controls)` extends the cold dry machinery without changing `dry::solve` or any
generated planet. The caller supplies **separate** metal/silicate host and protected
CO₂/N₂/CH₄/O₂/Ar inventories. Water and H/He are refused, as are secondary origins
in the host argument or refractory origins in the protected argument. No API
argument accepts exchangeable supply. Protection is a caller-supplied allocation,
not inferred from an `Inventory`'s molecular identity: passing exchangeable material
as protected violates the contract and cannot be detected from those masses alone.
The imposed load has no atmospheric inventory.

The approximation `entrained_secondary_host_volume_v1` assigns each trace origin
proportionally to host mass: `dm_i=(M_i/M_host) dm_host`. Every host parcel is
integrated with its **full** mass and `dV=(dm_host+Σdm_i)/ρ_host(P)`. This is neither
zero-volume ballast nor a repaired massless-host profile. The kernel's internal
metal/silicate route masses include their entrained supply; returned immutable
region inventories instead retain the original host and every secondary origin.
No trace shell is created. The returned `DryStructure` reports the entrainment
`model_id()` and the separate Seager `host_density_model_id()`.

This is an explicit local mechanical approximation, **not** a solubility law or
secondary-fluid EOS. Host and trace phase, temperature and chemical binding remain
unresolved. It establishes no molecular survival, stable condensate, exposure,
entropy, melting state or landing permission. It does not assign host entropy to
traces. Water hosts, pure/exposed secondary material and atmosphere partition remain
unsupported on this path.

Require positive host mass and `f=M_trace/(M_host+M_trace)≤0.00301`. The total parcel
mass must be 0.0001–20 Earth masses and every host pressure 0–10¹³ Pa. As for the
dry solver, some mass/load combinations cannot be bracketed within the pressure
ceiling and are explicitly refused. The same work ceilings and pressure-residual
policy apply. All supplied origins close to relative 10⁻¹²; region mass resolution
remains 10⁻⁸. Proportional allocations and differently grouped total sums can differ
by floating-point roundoff. An allocation that underflows a positive origin to zero
or an unrepresentable host interval is rejected, never silently dropped.

The fixed fixtures use host (metal,silicate)=(0.3,0.7) Earth masses and an imposed
outer load of 10⁵ Pa. At the mixed endpoint, set
`M_trace=f M_host/(1-f)` before applying the stated trace shares.
Seven regressions cover the six frozen acceptance groups: zero-trace recovery;
five origins individually at trace/host ratios 10⁻¹², 10⁻⁷ and 0.003; the mixed
f=0.00301 endpoint with shares (CO₂,N₂,CH₄,Ar)=(0.49,0.49,0.01,0.01); reversed input
order and repeatability; independent closure/refinement; density sensitivity;
invalid/domain/work refusals. Additional edge checks cover pure hosts, total mass
endpoints and unrepresentable allocations. Zero traces recover dry mechanics
exactly. All 17 fixed fixtures match the reference with host masses scaled by
`(M_host+M_trace)/M_host` to relative 10⁻¹² in radius, inertia and central pressure,
without relabelling returned origins. Independent radial Gaussian quadrature
checks total/per-origin mass, inertia and hydrostatic pressure drop, using the
same stricter numerical targets as dry validation.

Measured fixed-fixture residual maxima: region mass 1.44×10⁻¹⁰, pressure drop
1.82×10⁻¹⁰ and inertia 1.44×10⁻¹⁰. Fourfold step/tolerance refinement changes radius
by at most 1.32×10⁻¹³ and dimensionless inertia by 8.19×10⁻¹³, with at most 292,023
EOS calls in a refined request. These measure numerical consistency, not physical
accuracy. The mixed nominal result is R=6151.807097 km and I/(MR²)=0.3425255896.

The test-only sensitivity audit substitutes
`ρ_parcel=ρ_host/[1-f+f/λ]` for λ=0.5/1/2. Maximum radius change is 0.1184% and
dimensionless-inertia change 0.00994%, below the frozen 1%/2% gates. λ is not a
runtime parameter or evidence that actual unknown trace densities lie within that
range. Reproduce the fixed tests and measured diagnostics with:

```sh
cargo test -p space-simulation system::structure::dry::protected -- --nocapture
```

## Headless protected traces in water-bearing hosts

`system::structure::water::protected::solve(&host, &protected,
water_temperature_k, outer_pressure_pa, Controls)` extends the same
`entrained_secondary_host_volume_v1` approximation to metal/silicate/protected-water
hosts. Positive water is required. The caller supplies protected
CO₂/N₂/CH₄/O₂/Ar separately; H/He, secondary material in the host and host material
in the trace argument are refused. As for dry traces, the API cannot identify
exchangeable supply mislabelled by the caller. No atmosphere is assigned to the
imposed load. Existing `water::solve` and coupling behaviour remain unchanged.

Every parcel is integrated at its full host-plus-trace mass and the host's
specific volume. Proportional entrainment spans refractory regions and every
pressure-located water phase/model segment. The located parcel mass determines
that segment's host and per-origin trace allocations. No trace shell, massless
ballast, pressure shift or post-solve mechanical repair is introduced. Trace
inventory is not added to the gravity source a second time.

The returned `WaterStructure` exposes the entrainment `model_id()` and the original
water `host_model_id()`. Each region retains its host `density_model_id()`, phase,
temperature and approximation notes. **Region `phase()` and `temperature_k()`
describe the host only.** `origin_phase(material)` returns absent for an absent
origin and unresolved for each entrained secondary origin, even inside liquid
water or identified Ih. Trace thermal/chemical states, solubility, molecular
survival, freezing-point shifts and phase partition remain unresolved. This is a
prescribed entrainment model, not equilibrium mixing or simulated redistribution.

Require `f≤0.00301`, total mass 0.0001–20 Earth masses and all unchanged local
host P–T masks. The water temperature selects the **host** path; it supplies no
secondary phase law. Water event pressures and separate one-sided densities are
preserved without smoothing. The existing 0.01 Pa unshifted outer residual and
work ceilings apply, including every nested compressed-water EOS evaluation.
All origins close to species-local relative 1e-12 and region mass resolution
remains 1e-8. Unrepresentable host intervals or positive trace allocations that
underflow are explicit failures, not deleted inventory.

Seven regressions cover the frozen slice: zero-trace recovery at 250/260/300 K;
the five individual origins at trace/host ratios 1e-12, 1e-7 and 0.003 at 300 K;
mixed endpoint `f=0.00301` at all three temperatures; pure/thin-water hosts;
one-sided event loads, including Ih/liquid; deterministic/reversed inputs;
species-local conservation, independent quadrature/refinement and exact nested
work budgets; unsupported/domain/unrepresentable requests. The main host is
(metal,silicate,water)=(0.2,0.5,0.3) Earth masses with a 1e5 Pa load. The mixed
shares and trace-mass conversion are the same as the dry-host fixture above.

At nominal density, the profiles and event coordinates exactly match an ordinary
water solve with all host masses scaled by total/host mass, while returned origins
remain unchanged. Independent Gaussian radial integration closes region/per-origin
mass and total inertia to relative 1e-8 and hydrostatic drops to 2e-4. Across the
15 single-origin and three mixed endpoints, measured maxima are 4.66e-10 region
mass, 4.47e-10 hydrostatic drop and 5.77e-11 total inertia.

| Mixed endpoint | Radius (km) | I/(MR²) |
| --- | ---: | ---: |
| 250 K | 7317.751489 | 0.2873869592 |
| 260 K | 7318.684052 | 0.2873151794 |
| 300 K | 7308.116047 | 0.2880917845 |

Fourfold step/tolerance refinement on those endpoints changes radius by at most
1.88e-13, inertia factor by 3.86e-13 and individual region-origin allocations by
1.88e-11 relative, with at most 1,233,333 charged EOS evaluations. The test-only
λ=0.5/1/2 audit changes radius by at most 0.1221% and inertia factor by 0.00302%,
below the 1%/2% sensitivity gates. These are numerical/model-sensitivity checks,
not physical accuracy or a measured trace-density uncertainty interval.

Reproduce the frozen slice tests and diagnostics:

```sh
cargo test -p space-simulation system::structure::water::protected -- --nocapture
```

## Headless restricted water-shell solver

`system::structure::water::solve(&Inventory, water_temperature_k,
outer_pressure_pa, Controls)` is a separate opt-in API. It uses the same bounded
adaptive cube-root-mass kernel as the dry solver, not a second numerical
implementation. Its model identity is
`cold_refractories_isothermal_iapws_effective_ice_v5`. Generation, transport,
inspector and rendering do not use it.

The prescribed order is metal, silicate, water, omitting zero-mass regions.
Region-owned inventories conserve the retained input; positive water is required,
and every positive gas/other allocation is rejected. Total mass remains restricted
to 0.0001–20 Earth masses. Pure water is possible only where the entire body stays
within the supported pressure domain. Numerically unresolvable trace intervals
fail, rather than disappearing into a neighbouring region.

Water alone has a prescribed uniform temperature and resolved phase regions:
liquid, ice Ih or explicitly effective high-pressure ice, using the
[original low-pressure IAPWS model](water-physics.md)
between 1,000 Pa and 100 MPa, within 250–623.15 K. Below 250 K (down to 20 K) or
below 1,000 Pa the top water region instead uses the labelled
[surface-water model](water-physics.md#cold-and-low-pressure-surface-water), with
the cold intermediate and deep-ice branches below it. Compressed liquid additionally
uses IAPWS95 at 100–1,000 MPa and 273.16–623.15 K, restricted to its liquid mask.
Every stage and sample must remain in its selected model's domain.
Above 300 K, a separate labelled depth prescription supplies effective ice from
1 GPa; above 623.15 K, a bounded hot-fluid proxy supplies unresolved hot water
to 2,000 K below that boundary. These do not extend the original IAPWS APIs or
claim a thermal phase diagram. See the [hot/deep model and limits](water-physics.md#hot-and-warm-deep-prototype-extension).
An ice-Ih/liquid transition is supported only where **both** density
branches exist: 273.15–273.16 K, further restricted to melting pressures inside
the pressure mask. Pressure decreases outwards, so a crossing gives inner liquid
and outer ice Ih. The entire water allocation may also remain in one phase.
Vapour, ambiguous coexistence mixtures and unavailable subfreezing-liquid density
are refused. The prescribed ice-VII branch is not joined here.
Refractories retain cold Fe/MgSiO₃ density proxies with no assigned temperature
or established solidity. Temperature continuity across the refractory/water
interface is not asserted: this is a mechanical benchmark, not heat transport.

The pressure load owns no atmospheric mass and is not a climate prediction.
The returned radius is the outer condensed-material boundary at that load,
not necessarily a one-bar radius, exposed rock or landable ground. Radius,
enclosed mass and pressure agree exactly across composition interfaces, with
separate one-sided densities.

At 100 MPa the liquid EOS changes from inner IAPWS95 to outer IF97. This is a
**model boundary, not a phase change**: two liquid regions carry disjoint water
allocations and distinct density-model identities. The same pressure-coordinate
event integration described below preserves one-sided densities without blending,
offsets or forcing the EOS values to match. The small unforced density mismatch
may have either sign. An outer boundary at 100 MPa uses the inward IAPWS95 limit,
without a zero-mass IF97 skin. The supported dense-liquid and Ih-transition
temperature masks do not overlap at supported interface pressures, so each
earlier restricted requests needed at most one water discontinuity. The kernel
now locates ordered pressure events, including a deep-ice boundary followed by
the liquid-model seam. Each newly located enclosed mass starts the next disjoint
allocation, rather than reusing the start of the whole water shell.

Phase interfaces lie on the nominal IAPWS Ih melting curve. Integration approaches
the curve from the high-pressure side, then finishes the last ≤1 Pa in pressure
coordinates using the same embedded Runge–Kutta method. Prescribed endpoint
pressure is the independent coordinate, not a pressure correction to a mass-step
solution. Radius, enclosed mass and inertia evolve to that endpoint; both
one-sided densities are evaluated there before the outward branch changes.
No accepted step traverses the density jump and no EOS stage leaves its stable
branch. The 1 Pa coordinate-switch threshold is numerical, not a phase tolerance
or a physical interface offset. Embedded error, maximum mass-coordinate step and
request work limits still apply, including rejected interface probes.

The interface's enclosed mass determines the inner water allocation; the remaining
water belongs to the outer region. There is no hidden third mixture reservoir.
Unresolvable phase allocations fail the existing mass-resolution contract rather
than being dropped or merged. A coexistence outer boundary uses the inward liquid
limit, with no fabricated zero-mass ice skin. The local unconstrained `water::state`
call still refuses a coexistence density; explicit named one-sided limits use
`water::condensed_state`.

Shooting requires an unshifted outer pressure residual between zero and
**0.01 Pa**, independent of central pressure. Integration pressure-error scaling
is also independent of central pressure: `relative_tolerance × 100 MPa`
(0.01 Pa at default tolerance), including within the refractory interior.
This is an embedded local error target, not a global error bound or a physical
uncertainty. Defaults and work ceilings are shared with the dry solver. Unsupported
high-pressure/phase trials constrain the upper shooting bracket but never become
successful partial profiles. Their attempted steps and EOS calls count towards
the request budget. Density dispatch, initial outer-state validation, the IF97
inversion seed and every nested Helmholtz pressure probe are charged, including
failed inversions and rejected stages. Each compressed-liquid call has a fixed
99-evaluation ceiling (one seed and up to 98 pressure probes); dispatch costs one
additional request evaluation. Budget exhaustion and inversion bracket/root
failures propagate, not physical upper brackets. No work ceiling is raised.

Thirteen headless tests cover an analytic three-layer constant-density benchmark,
independent radial mass/inertia/hydrostatic quadrature, conserved liquid/ice
shells and pure-water endpoints, deterministic repeats, imposed loading,
refinement, one-sided interfaces, EOS/phase/domain refusals and exact EOS-budget
exhaustion. Fixtures include 0.0001-Earth-mass bodies with 1% and 100% water and
one-Earth-mass bodies with 10⁻⁶ water mass fraction, each at 250/300 K.
Independent quadrature closes region mass and total inertia to relative 10⁻⁸
and hydrostatic drops to relative 2×10⁻⁴. Tightening integration tolerance and
maximum step by four changes radius, inertia factor and central pressure by
less than relative 10⁻⁷, and water-shell thickness/pressure drop by less than
relative 10⁻⁵ on these fixtures. Phase-interface tests use the same mass/fraction
fixtures at 273.15, 273.155 and 273.159 K with 1,000 Pa outer pressure; independent
quadrature meets the same closure targets. At 273.155 K, fourfold refinement
changes each water-region allocation, thickness and pressure drop by less than
relative 10⁻⁵. An independent two-density analytic sphere checks the located
interface radius and mass, total radius and inertia to relative 10⁻⁷. Endpoint
and budget tests include a trace phase too thin to resolve, which fails explicitly.
Compressed-liquid tests additionally cover an Earth-mass, 0.5%-water shell at
300 K crossing 100 MPa; loaded 0.0001-Earth-mass, 1%-water bodies at 500 K
(100, 700 and 990 MPa loads); and a 273.16 K model-endpoint fixture. Independent
mass, inertia and hydrostatic quadrature meets the same targets. Fourfold
refinement of the Earth-mass crossing changes radius, inertia factor and central
pressure by less than relative 10⁻⁷ and each region's allocation, thickness and
pressure drop by less than 10⁻⁵. Exact-budget repeats include nested work and
initial dense-state exhaustion. The 0.0001-Earth-mass pure-water fixture with an
80 MPa load at 300 K exhausts the existing request EOS budget and is explicitly
refused, not accepted with uncounted work. The former 1 GPa/500 K ceiling refusal
now solves on the separately labelled warm effective-depth branch.
These are numerical consistency targets only. The melting curve's 2% published
pressure uncertainty remains, regardless of exact numerical endpoint placement.

```sh
cargo test -p space-simulation system::structure::water
```

### Effective deep ice and checkpoint-3 acceptance

At **250–300 K**, deep water uses Seager et al. (2007), equation 11/table 3:
`ρ = 1460 + 0.00311 P^0.513` in SI. Model
`seager_2007_effective_high_pressure_ice_v1` represents compression only, not
resolved VI/VII/VIII phases, Gibbs potentials or a thermal EOS. The independently
selected IAPWS V/VI melting curve supplies its approximate liquid boundary
at ≥273.15 K. Colder paths use the intermediate-ice prescription below.
This composite prescription preserves a density jump but does not claim matched
thermodynamic potentials or accurate ice-VI density near 1 GPa. See the
[material limitations](water-physics.md#effective-high-pressure-ice-for-the-game).
The application ceiling is 10¹³ Pa. All stages remain within the selected branch;
the original local equilibrium APIs and coexistence refusals remain unchanged.
An explicit budgeted liquid-side limit supplies the melting endpoint density.
An outer load on a boundary uses the inward branch, with no zero-mass skin.

The mandatory retained inventory is 0.2 metal, 0.5 silicate and 0.3 water in
Earth-mass units, at 300 K and imposed 1 bar. It yields **7,301.552 km** radius,
**322.444 GPa** central pressure and **0.28809616** inertia factor. Its five
regions are metal, silicate, effective deep ice, IAPWS95 liquid and IF97 liquid.
It uses 938,500 charged EOS evaluations, below the unchanged 2,000,000 ceiling.
These numbers describe the prescribed approximation, not an observed planet.

Twenty radial tests retain all shallow-water checks and add independent deep
mass/inertia closure (relative 10⁻⁸), hydrostatic closure (2×10⁻⁴), deterministic
repeats, fourfold refinement (radius/inertia/central pressure 10⁻⁷, region mass
and thickness 10⁻⁵), loading through 2 GPa, water-fraction and compression trends,
and exact-budget success/failure. No tolerances or work ceilings were relaxed.

The matching **250 K fixture now also solves**, using rounded engineering ice
boundaries at **220 MPa** and **2.1 GPa**. Ordinary IAPWS Ih is unchanged up to
100 MPa; a separately identified pressure extension covers 100–220 MPa. Effective
intermediate ice spans 220 MPa–2.1 GPa; deeper ice uses the same Seager proxy.
The intermediate Murnaghan compression law is anchored to the existing
1.5 GPa/293 K H₂O lattice density, with assumed elastic constants and omitted
thermal corrections. It does not assert resolved II/III/V/VI/VII/VIII shells.
Each approximate region exposes `approximation_notes()` as well as its distinct
model identity; precision integration does not imply precise phase physics.

The cold fixture yields **7,311.198 km** radius, **322.427 GPa** central pressure,
**0.28739002** inertia factor and 360,824 charged EOS evaluations. Its six regions
include two Ih density-model regions, not two different Ih phases. Independent
closure, fourfold refinement, exact work accounting, pure-water/fraction fixtures,
and boundary loads through 2.1 GPa pass the same targets as the warm path.
The all-solid cold path covers **250–251.165 K**, below the minimum melting
temperature, not just one isolated temperature. Above that, IAPWS Ih and III/V
melting curves locate the pressure-melted liquid interval. A labelled approximate
liquid-compression law supplies its density below the original IAPWS temperature
masks. The original IF97/IAPWS95 models remain preferred wherever available;
their local APIs are not silently extended. Tests span the former gaps and
branch-temperature endpoints, with independent closure and a 260 K refinement
fixture. The specified water-rich inventory now solves across **250–300 K**,
without classifying the intervening liquid regime as ice. This does not remove
the declared pressure/mass bounds, vapour refusal or deterministic work limits.
Near the rounded Ih/III triple point, liquid wedges no wider than 10 kPa are
explicitly unresolved and use the all-solid approximation. This avoids inventing
an inverted or infinitesimal shell from rounded source endpoints, without
discarding any water inventory.
**Checkpoint 3 is complete under its plausibility-focused scope.** Atmosphere
coupling, generation/consumer migration and thermal evolution remain later work.

## Headless atmosphere and condensed-boundary coupling

`system::structure::coupled::solve(&Request, Controls)` implements layered
checkpoint 4 as a headless API. The background solver now uses it where supported;
its results can reach live consumers through the [adoption barrier](#safe-background-adoption).
The physical solver itself introduces no camera restrictions.
The model is `loaded_condensed_spherical_isothermal_column_v1`.

The immutable request supplies retained inventory, a subset eligible for gas
exchange and a `Temperature`: either a prescribed column temperature and (when
needed) water temperature, or an equilibrium temperature for the
[greenhouse surface temperature](#greenhouse-surface-temperature). Exchangeable
supply is an explicit exposure prescription, not an extra allocation. Source and
escaped bookkeeping stays with the caller. Formation/accessibility priors are
not run inside this API. Gas is partitioned using the same approximate cold-trap
caps as generation. The remaining metal, silicate and water belongs to radial
condensed regions; neither a mutable reservoir copy nor a second column owns any
mass. Exchangeable CO₂, N₂, CH₄ and Ar above their cap form a
[surface deposit](#frost-surface-deposit). Exchange smaller than
one machine epsilon of that species' own retained total (for example a few kg of
vapour over a cold ocean) cannot change the condensed total in f64. It is
accepted: the gas keeps it and the sum still closes exactly. The allowance is
species-local, never scaled to planet mass, and larger transfers that fail to
debit their source are still refused.

The fixed-point iteration matches three quantities:

1. Solve supply-limited mixture and finite spherical column mass at a trial
   radius and mass under the gas (condensed body plus deposit).
2. Apply its inventory-derived base pressure, plus the deposit's weight, to the
   dry or water radial solver, using the remaining metal/silicate/water inventory.
3. Re-evaluate pressure, load and partition at that solved radius and mass. Repeat
   until gas pressure and total load each differ by at most **0.01 Pa** and each gas constituent's
   allocation differs by at most relative **10⁻⁹**, scaled to that constituent's
   gas allocation, not planet mass.

The ambient-density radius is only an iteration seed, never a returned fit or
fallback. Accepted gas pressure is normalised to actual gas allocation at the
final radius. The radial unshifted residual is also at most 0.01 Pa, giving an
interface mismatch of at most **0.02 Pa**. The dry solver uses a coupling-only
absolute-pressure policy; its standalone API keeps its original policy. No
pressure-profile translation repairs a solve. Region plus gas allocations derive
retained totals with species-local relative 10⁻¹⁰ closure.

Total inertia includes the condensed integral, the deposit shell and spherical gas integral
`I_gas=(8π/3)∫ρr⁴dr`. Its inertia factor uses total retained mass and the
**condensed boundary radius**, not one bar or optical extent. The gas column
starts at the top of the deposit (the condensed boundary when there is none).
One bar is located only if it exists above that base; otherwise it is absent. Boundary pressure
is never a request input. Airless differs from trace gas: this API has no 0.1 Pa
display cutoff.

### Coupling domain and explicit approximations

- Total and condensed masses must both fit the radial 0.0001–20 Earth-mass domain.
  Supported condensed materials are metal, silicate and checkpoint-3 water.
- Gas is at most **1% of retained mass**. Self-gravity is neglected: column
  normalisation and pressure profile consistently use the condensed plus deposit
  mass, not a point mass containing the gas shell. This is an engineering restriction,
  not a planet classification.
- Column temperature, prescribed or warmed, stays within **20–2,000 K**,
  generation's own floor.
  Below 50 K the cold-trap caps are extrapolated from their liquid-vapour
  anchors and so overstate the frost vapour pressure; O₂ and H/He have no cap and
  stay gas. Both are labelled notes, not a separate solver. Base ideal-gas density
  cannot exceed **10 kg/m³**, an engineering mask, not an established nonideal
  error bound. A separate **bounded hot ideal-steam approximation** allows up to
  **50 kg/m³** only at **1,000–2,000 K** with **at least 90% water by mass**.
  `column_model_id()` identifies `bounded_hot_ideal_steam_column_v1` on that
  composition/temperature route. It still uses the ideal-mixture equation, not a
  new real-fluid EOS: nonideality and dissociation are omitted and no physical
  accuracy bound is claimed. Colder steam, dense CO₂ and dense H/He do not gain
  the wider density mask. The 1% gas-mass and sonic-radius guards are unchanged.
  Pressure/mass and inertia quadratures now use 512 rather than 256 steps to
  preserve independent closure on these higher-pressure columns.
- The existing spherical sonic-radius guard must pass. Weak binding returns an
  unsupported error instead of accepting the legacy exponential fallback as a
  coupled solution. Extended columns are not rejected solely for their height.
  The separate [blow-off wrapper](#unbound-column-blow-off) resolves it by escape.
- The entire gas allocation normalises the operational finite shell described in
  [atmospheres](atmospheres.md#pressure-and-cold-trap-solve). No additional tail
  mass is inferred. Its cutoff is a prescription, not an exobase.
- Water keeps the radial model's bounds. Ice surfaces may sit at any pressure,
  including airless ones; a liquid surface needs at least its saturation
  pressure, otherwise the vapour is refused. No pressure floor is invented.
   Prescribed gas and water temperatures are independent imposed anchors: unequal
  values describe mechanical loading, not thermal or evaporation equilibrium.
  Under the greenhouse both take the one surface temperature.
- Cold-trap caps are approximate, not matched Gibbs or chemical equilibrium.
  Exchangeable secondaries above their cap become a surface deposit. Water above
  273.16 K is capped at the IF97 saturation pressure instead, so a column
  saturated in water leaves the liquid below it stable; ice keeps the cold-trap
  fit. The condensed load is computed on the geometry the gas was partitioned on,
  so a pure steam column loads its ocean at saturation to rounding, and the ocean
   top takes the liquid-side limit. Protected water is supported by the low-level
   API; protected secondary condensates require the unified entry point below,
   which integrates them with the protected-trace host prescription.

### Greenhouse surface temperature

`Temperature::Greenhouse { equilibrium_k }` replaces the imposed temperature
with the [grey greenhouse estimate](atmospheres.md#grey-greenhouse-estimate).
The isothermal column and the condensed water both take the warmed surface
temperature `T_s`.

- **Solved on the coupled column.** At each coupling geometry `T_s` balances
  `T_s = f(T_s)`, where `f` warms `T_eq` by the optical depth of the gas
  partitioned at `T_s`. Warming raises the caps, so frost and water evaporate,
  which warms further.
- **The coolest balance.** A search up from `T_eq` finds the first crossing,
  then refines that bracket. Each step is the larger of 0.5% and half the
  excess `f(T) - T`. `f` mostly rises with `T`, but a supply-limited column
  thins slightly as it warms, so `f` can fall: by at most 0.07 K per K in the
  399-system probe. The half step stays safe while `f` falls no faster than
  `T` rises; that is an assumed bound, checked on the probe, not a proof.
  Refinement is false position with Illinois weighting, and a bisection step
  whenever two steps fail to halve the bracket. It stops at relative
  temperature width 10⁻¹², three orders tighter than the coupling test, or an
  exact zero. As before, balances closer than a scan step may be missed. This is the balance a planet warming up from equilibrium would
  reach. Hotter balances, which a planet that started hot (a magma-ocean steam
  world) could keep, are not explored.
- **The unwarmed column is checked first.** If the column at `T_eq` is already
  dense or weakly bound, that error is returned, so a dense H/He column is still
  reported as one and not as a runaway its broadening would drive.
- **Runaway is refused, not capped.** If warming passes 2,000 K or pushes the
  gas past 1% of the mass before balancing, `WarmingOutsideDomain` is
  returned. A balanced column that is then too dense still fails the usual
  dense-gas check, including the separate bounded hot-steam mask.
- Convergence also requires `T_s` to change by at most relative 10⁻⁹ between
  geometries. Results report `surface_temperature_k` and
  `greenhouse_optical_depth`.

### Unbound column blow-off

`coupled::escape::solve_escaping` (`unbound_column_lightest_first_blowoff_v1`)
wraps `solve`. A static column reaching past its Parker sonic radius would flow
outward, and exposed supply (an ocean, say) keeps refilling it, so over
geological time that gas is lost. On `WeakBindingUnsupported` the wrapper
removes the **lightest exchangeable gas entirely** from both supplies, reports
it as `escaped` and re-solves, until the column binds or no gas is left.

- Only exchangeable supply escapes. Condensed water outside the exchangeable
  supply, protected secondaries and refractories stay.
- Order is by molecular mass (H₂, He, CH₄, H₂O, N₂, O₂, Ar, CO₂), a crude
  stand-in for diffusive fractionation. Hydrodynamic drag of heavier species
  is not modelled; neither is any escape rate, timescale, energy limit or
  isotope effect. A bound column loses nothing, however hot.
- Every other refusal (dense gas, gas mass, runaway) is returned unchanged.
- Each attempt carries the full controls, at most one per gas species, so
  worst-case work is nine solves, not one shared budget.
- `structure + escaped` closes on the request's retained inventory.

It is a second escape on top of generation's population-level secondary escape,
not a replacement for it.

### Frost surface deposit

Cold-trapped CO₂, N₂, CH₄ and Ar settle on the ground as ice, as on Mars, Pluto
and Triton. The excess above each gas's vapour-pressure cap is a labelled
deposit region on top of the rock or water, not atmosphere mass. The air above
it stays at the frost's vapour pressure. O₂ and H/He have no cap here, so they
never freeze out.

- **Fixed solid densities, not compressed:** CO₂ 1,560, N₂ 1,030, Ar 1,620 and
  CH₄ 520 kg/m³. Mixed frost uses the volume-weighted bulk density.
- **Thickness** is the shell that holds that volume. Its exact hydrostatic
  weight (a uniform self-gravitating shell on the condensed mass) is added to
  the gas pressure as the condensed body's outer load. The rock and water
  solvers are unchanged.
- **Simplifications:** one even global layer, though real frost gathers at the
  poles or nightside. Placement on top is a convention (CO₂ ice is denser than
  water ice). The phase is not modelled: CO₂ above about 5 bar and 217 K would
  be liquid, but solid density is used. Dissolution in oceans is ignored.

Only about 10% of each secondary is accessible, so deposits are a small share
of mass. Their thickness describes the surface; their effect on radius is small.

Dense envelopes retain the explicitly identified existing `EnvelopeSurrogate`
generation model. A failed coupled solve never automatically becomes a successful
surrogate. A future dense-mixture EOS and thermal prescription can replace that
approximation. No resolved metallic hydrogen, helium core or giant entropy
profile is claimed.

### Numerical work and acceptance

At most **32 coupling iterations**, each with two 64-step partition evaluations,
additional partition evaluations for the greenhouse search, and finite-column
pressure bisections, are permitted. The aggregate ceiling is
**16,000,000 radial EOS evaluations**, including nested work and retries. Each
radial call retains its existing 2,000,000 maximum and other ceilings. Remaining
aggregate budget limits each subsequent call; a failed call terminates the
request, never retries with reset work (the envelope route's one
[bisection retry](#headless-dry-body-solver) excepted). Controls can lower ceilings, not raise
them. Nonconvergence is explicit.

After the first interior solve, coupling carries forward its central pressure
as a **search hint**, not a reused profile. The radial solver reintegrates at
that pressure with the new inventory, load and water temperature. If it is not
already acceptable, a local bracket expands towards higher or lower pressure
until it encloses a solution. The initial span is relative 10⁻⁶ of the hint
(at least 10⁻⁵ bar); this affects search cost only, not acceptance. Invalid or
out-of-range hints use the original unseeded search. Standalone and protected
host solves still start unseeded.

All trial integrations, boundary tolerances, phase/domain checks and work
ceilings are unchanged. Water-domain failures may delimit an upper bracket
exactly as before, but numerical/budget errors propagate. Hint probes debit
the same request budgets, without reset or a hidden fallback. Blow-off attempts
start fresh, so a different retained inventory never inherits another attempt's
hint.

```sh
cargo test -p space-simulation system::structure::coupled -- --nocapture
```

Twenty-three tests cover thin nitrogen, cold/warm 30%-water bodies, protected water, trace
and absent gas, mixed N₂/CO₂, thin H/He, N₂ frost on rock, CO₂ frost loading a
warmer water host, a 30 K icy body with N₂ frost on airless ice, an ocean under its own saturated steam and trace water fully evaporated, an
Earth twin warmed from frozen to a liquid ocean, unwarmed transparent and absent
air, a steam runaway refused, bounded hot steam, lightest-first blow-off, sub-ulp vapour, stripping, deterministic repeats,
refinement (including bracket preservation and interpolation versus full-precision
bisection), accurate/inaccurate/invalid dry and water interior hints with exact
work-limit checks, exact aggregate-work success/failure and unsupported inputs/regimes.
Independent radial quadrature closes shell mass and inertia to relative 10⁻⁷
and hydrostatic pressure drop to 10⁻⁹. Fourfold refinement changes radius,
pressure and water allocation by less than relative 10⁻⁷. These are numerical
targets, not scientific error bars.

The nitrogen fixture (0.325 metal, 0.674999 silicate and 0.000001 nitrogen Earth
masses at 250 K) takes three iterations to reach **6,113.588 km** and
**134,987.088 Pa**. Replacing 0.3 Earth
masses of silicate with exchangeable water gives **7,179.896 km / 70,950.716 Pa**
at 250 K and **7,170.596 km / 73,548.551 Pa** at 300 K. Both take three iterations.
These are prescribed
test compositions, not reconstructions of observed planets.

The cold fixture exposed a round-trip floating-point error: `(P/scale)*scale`
could lie one ulp beyond an outward EOS branch. The shared kernel now recognises
**exact equality** to a prescribed event's dimensionless coordinate and evaluates
the original endpoint pressure. No tolerance band, pressure offset, density
blend or EOS-domain extension is used.

## Experimental background loading

Background refinement is enabled by default. Pause it under F3 → perf →
planet refinement, with `ns planets off` or `set_layered_planets false`.
The switch resets on at restart. Completed results reach the inspector and,
when [safe](#safe-background-adoption), become authoritative for rendering,
shape, optics and navigation. A refusal retains the initial partitioned
[fallback structure](#fallback-structure), labelled
"Fallback (layered unsupported: …)" with its typed reason, never shown as a
successful solve.

### Layered transport record

`system::physical::layered` turns a solve into `space_shared::LayeredPlanet`,
held inside a `PlanetResult` for adoption, or beside the active record while
deferred/unavailable. The active `Planet::physical` is always its result's verbatim
physical projection.

- `physical` keeps the `PlanetPhysical` shape. Reservoirs map as interior =
  condensed regions minus water, condensed = condensed water plus the frost
  deposit, gas = the coupled gas inventory. `reference_radius_m` is the top of
  the surface, frost included, on the thin route. Envelopes instead use the
  explicit one-bar level, with gravity from mass enclosed there and total
  inertia referenced to that radius. The full retained mass includes the outer
  envelope shell as well.
  `approximations` lists the model IDs used (prototype, condensed structure,
  column and each region's density model), not legacy caveat IDs.
- `radius_reference` is `solid_surface` only for bare rock or bare ice-Ih. An
  ocean, a frost deposit (phase not modelled) and unresolved water stay
  `unresolved_volatile_boundary`. This older reference enum is geometric, not
  certification of refractory solidity or landing permission; the new
  [result contract](#authoritative-result-contract-and-validation) carries
  separate surface knowledge. Envelopes use `one_bar`, never `solid_surface`,
  and label reference temperature/pressure accordingly in the inspector.
- `layers` run outermost first: the atmosphere's operational finite shell (its
  top pressure is not resolved, so absent), the frost deposit, then each
  condensed region named by phase (`ocean`, `ice`, `deep_ice`, `hot_water`,
  `water_coexistence`) or, for refractories, by its dominant material (`mantle`,
  `core`). Cold refractory regions carry no temperature.
  Envelopes instead begin with numerical `envelope` segments separated at the
  one-bar event, not a phase or radiative/convective interface. Loaded water is
  `water_compression` with unresolved phase.
  Their temperature fields are outer-boundary values, not isothermal claims.
- Layer masses sum to the retained mass and adjacent layers share their radius.
  The coverage report checks both, and the surface label, for every solved planet.

The inspector's **Background refinement** section compares radius,
surface temperature and pressure with the active values, and notes that the
planet is still drawn at its active radius. It then lists greenhouse optical
depth, blow-off losses, one row per layer (thickness, mass, pressure range,
temperature) and the models used. An applied candidate instead says
**Applied · active result**, without pretending it is still an alternative.
The section is absent while the mode is off.
The object browser's detail pane does not show it.

`system::generated::generated_system_with_mode` defaults via the existing
`generated_system` wrapper to `Mode::Synchronous`, using
the synchronous fallback. `Mode::PrepareRefinement` also
prepares immutable per-planet budgets, environments and starting results, without solving them
inline or changing the initial wire system. The client prepares these inputs
off-thread by reconstructing the deterministic natural system once, avoiding
transport-record decoding or changes to live snapshots. This duplicates the
cheap initial generation on a cache miss, not the expensive physics solve.
The optional list/task overlay is not solved separately; its matching natural
planet receives the same adopted projection, preserving page/task metadata.

The current visited system is the target (including during a navigation
flight). Without a visit, a selected star or remote planet's parent system is
prefetched. A lingering selection of a system just left does not restart its
cancelled queue. Each planet is one independent background job; the selected
planet goes first among queued jobs. Jobs already running are not reordered.
The [shared star/search/planet budget](../client/streaming.md#shared-background-cpu-budget)
prevents independent worker allocations from competing unchecked. Refinement
starts only while visible-star streaming and object search are idle; running
solves retain their permits until they finish.

Results live in a four-system, least-recently-targeted in-memory cache, keyed by
stable natural-system ID, with captured per-planet input/model revisions and
target epochs. Completed solves
and fallbacks are reused on revisit; incomplete entries prepare only their
missing jobs. There is no disk cache or cross-version reuse. Changing targets or
disabling the mode cancels queued work, marks running work obsolete and rejects
its results using a request epoch. Running numerical calls finish under their
unchanged work ceilings before releasing their slot; they are not forcibly
interrupted. Worker panics/spawn failures are explicit diagnostic failures.
The frame thread drains at most two result messages per update and never joins
unfinished workers or waits for a budget permit. Closing the debug window does
not stop jobs. Conversion to the transport record runs on the worker too. The
inspector rebuilds facts when status, adoption reason or active revision changes.
Loaded records take priority; remote generation replays any session-retained
adoption before reporting facts.

The original fast system-generation path remains synchronous, with an info-level
timing log at the client generation wrapper. Two headless sample systems took
5.6 ms (19 planets) and 1.4 ms (9 planets) in the optimised test profile. These
are preliminary samples, not an interactive frame-time guarantee or a population
benchmark; moving this initial path off-thread remains warranted if flight logs
show hitches. The debug readout separately times background input preparation.
The user runs visual/frame-time/star-flight benchmarks; no such test has been
launched as part of this implementation.

## Fallback structure

`structure::prototype::resolve` returns the layered solve, or, when
`prototype::solve` refuses, `structure::fallback::solve` with the refusal reason.
`system::physical::outcome` converts either to `LayeredPlanet`; fallback records
carry `model: fallback_mass_radius_v1` and `fallback_reason`, and never count as
layered coverage. The fallback is total by construction: closed-form fits and
fixed quadrature, no iteration, work budget, external data or error path.

- **Condensed stack**, centre outwards: metal core, silicate mantle, water,
  then secondary volatiles (N₂, CO₂, CH₄, O₂, Ar) at a water-density proxy.
  Each region's volume is its mass share of the Seager et al. (2007) pure-
  material radius at the whole condensed mass, as in the
  [cold compressed radius](#cold-compressed-heavy-material-radius), with the
  same power-law continuation above 20 Earth masses (labelled).
- **H/He envelope**, when present: the one-bar radius from the envelope fit and
  thin-column support limit
  ([light envelopes](#light-envelopes-and-the-giant-transition)). Sub-bar gas
  is represented as one isothermal scale height (capped at the condensed
  radius) above a condensed reference.
- **Profiles:** uniform density per condensed region; gas density falling
  linearly to zero at its top. Pressures integrate hydrostatically from zero at
  the top; inertia uses the same profiles. A full linear-profile sphere has
  inertia factor 0.267, close to Jupiter's 0.254.
- A positive trace constituent can add less than one radius ulp, leaving
  coincident boundaries. Keep its inventory and use the zero-thickness shell
  limits `I=2mR²/3` and `ΔP=Gm(M_inner+m/2)/(4πR⁴)`, with a limitation note,
  instead of dividing by zero volume or inflating geometry. Nonzero-width shell
  calculations are unchanged.
- **Inputs:** non-finite/negative irradiation and non-finite/non-positive age
  take neutral fit values. Zero irradiation is valid and reaches the fit's
  lower calibration bound. Flux/age repairs, non-finite temperature and fit-bound clamping mark the
  record `extrapolated` with a limitation note; otherwise it is `approximate`.
  Temperature is the equilibrium estimate (at least 20 K), with no greenhouse.
- **Surface:** `envelope` at a one-bar reference, else `rock` (nominal boundary,
  not certified solid phase or landing permission),
  `unresolved water` or `unresolved volatiles`.

Checks (`structure/fallback/tests.rs`, in `check.sh`): an Earth twin within 5%
of Earth's radius with inertia factor 0.30–0.36; a Jupiter twin at 9–13 Earth
radii with inertia factor 0.2–0.3; every single constituent at 10⁻⁶, 1 and
3,000 Earth masses under NaN/negative environments; 20,000 random inventories
from 10⁻⁶ to 10⁵ Earth masses with missing constituents and hostile
environments. Each asserts exact per-material closure, contiguous layers from
the centre, finite positive radius, density, gravity and inertia (at most 2/3),
and monotone non-negative pressures. The coverage report also checks every
census refusal's fallback record.

Limitations: no separate secondary atmosphere (the partition below adds one),
phase, thermal structure or greenhouse. For bodies with under 1% H/He, fallback radii exceed legacy (which
treats that gas as a thin column) by up to 61%; layered solves of similar
bodies exceed legacy by a median 32%, so the legacy value is the outlier.

### Synchronous reference partition

`physical::initial_with_atmosphere` prepares the initial `PlanetResult` with its
matching reservoir budget and `PlanetEnvironment`. It starts with the converged
retention fallback and applies `fallback_reference_partition_v1`, without a
legacy structure/boundary solve, EOS access or re-running retention. Retention
installs all of it as the [live projection](#live-projection).

This bounded approximation deliberately holds the fallback's condensed proxy
volumes, radius and reference meaning fixed. Accessible water/secondary mass
enters gas through the existing source-bounded cold-trap prescription; protected
material and prior escape remain accounted for. Condensed densities, loading
pressures and inertia are recomputed for their reduced masses, not left at the
pre-partition values. Fixed proxy volumes are a disclosed approximation, **not**
a coupled phase/volume, greenhouse or thermal-structure solution.

- A condensed reference gets one finite `atmosphere` region, normalised to the
  actual gas inventory. Its pressure/temperature match the prepared atmosphere.
  Inertia integrates the same spherical-isothermal or explicitly unresolved
  exponential profile, normalised by mass. The finite support uses the current
  column cutoff; it is not a material edge or independent visibility detection.
- A one-bar reference keeps one prescribed bulk `envelope` region, with secondary
  gas included in its inventory. The optical column samples it, not an additional
  atmosphere mass. The bulk profile and deep pressure remain the labelled
  thinning prescription, not an isothermal extrapolation to the core.
- Boundary gravity retains fallback's total-mass central-field approximation;
  atmospheric self-gravity is not newly solved. Weak-binding pressure retains the
  explicit planar approximation even though inertia uses a normalised finite
  profile. These numerical conventions do not validate a static atmosphere there.
- Positive trace gas below the live 0.1 Pa presentation threshold remains
  represented in the result, including its nonzero pressure. Vapour transfers
  below a reservoir ulp remain condensed and are excluded from the pressure
  solve, so unrepresentable vapour cannot change the mixture used to satisfy
  saturation caps.

The standalone `fallback::initial`/`solve` prescriptions and archived P2 numerical
outputs are unchanged. The new partition is applied once to an unpartitioned
retention fallback; repeated preparation from the same input is deterministic,
and repartitioning its output is refused. The prepared atmosphere passes the
checked optical adapter against its own result.

## Prototype coverage report

An opt-in report measures how much of the generated population the
rock-and-water prototype path solves, so each implementation step of the
[plan](../plans/planet-composition.md) can be measured against it.

```sh
cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture
```

`NS_COVERAGE_SYSTEMS` sets systems per galactic component (default 20, using the
census keys `composition-calibration:{component:?}:{index}`).
`NS_COVERAGE_THREADS=1` gives uncontended per-planet timings.
`NS_COVERAGE_TOLERANCE` overrides the radial relative tolerance (10⁻¹³ to
10⁻⁶) for sensitivity runs; generation always uses 10⁻¹⁰. The summary ends
with a stage profile: inclusive time and calls per planet for coupled
attempts, coupling iterations, radial trials, greenhouse searches, column
partitions and envelope solves, by route. That timing is compiled only into
test builds. The summary is
printed and written to `data/tmp/composition-coverage.md`, with one row per
planet in `data/tmp/composition-coverage.tsv` for diffing between steps.
Use `NS_COVERAGE_THREADS=20` for comparisons on the development machine.

Every refusal also runs the fallback structure and checks its record. The
summary's Fallback section counts checked fallbacks and their radius difference
from the synchronous generated reference (`initial_radius_km` and `initial_pa`
identify that basis in new TSVs; archived P2 TSVs retain their historical
`legacy_radius_km` and `legacy_pa` names). The TSV's last column, `fallback_radius_km`, is `NaN` for layered
solves.

The same run writes `data/tmp/composition-census.md` for P2 requirements:
envelope first blockers, generated classes, intersections of mass/H/He fraction/
equilibrium-temperature bands, composition bounds and deterministic regression
candidates. It lists every non-envelope refusal as well. The TSV appends
separate hydrogen, helium and refractory mass fractions, H/He and non-H/He
masses in Earth masses, and the helium mass fraction within H/He (blank when
absent). These are retained **input** inventories, before prototype blow-off,
not calculated envelope allocations. Non-H/He includes water and secondaries,
so it must not be mistaken for a rock/metal core. Diagnostic bins are not new
solver thresholds. The census cannot infer deep temperatures or pressures,
mixing, envelope radii or failures hidden behind the first blocker.
Raw equilibrium temperatures may be below 20 K; the prototype still applies
its existing 20 K floor. The P2 census and selected starting fixtures are
summarised in the [delivery plan](../plans/planet-composition.md#step-1-generated-input-census-complete-28-october-2026).

The report uses `structure::prototype::solve`, the same entry point used by
background refinement. Synchronous generation uses the
fallback, without running this layered solve inline.

The consumer-migration regression rerun checks **1,437/1,544 layered (93.1%) plus
107/107 labelled fallbacks**, unchanged in total outcomes. It is the standard
140-system calibration probe, not a new 399-system run or a whole-population
accuracy claim. At that consumer-migration checkpoint, full checks passed
474 simulation and 380 client tests, plus the
22 source-backed/data-free envelope fixtures. The saved audit reconstructs all
315 fallbacks with unchanged radii; the accepted diffuse extreme retains its
tighter-refinement work refusal. Evidence:
`data/tmp/p3-consumers-{check,coverage,envelope,audit}.log`.

The subsequent adoption slice passes **478 simulation and 391 client tests**, the
16 CLI tests, formatting/strict Clippy and all 22 envelope fixtures. Its safety,
cache/revisit and inspected ice-boundary regressions are recorded in
`data/tmp/p3-adoption-{check,envelope}.log`. It adds no new EOS prescription or
population-coverage claim. Targeted user visual/interactive acceptance is deferred
to [P4](../plans/planet-composition.md#p4-targeted-interactive-adoption-acceptance)
by user decision; P3 is now closed after legacy cleanup and its automated checks. The
deferred checks are not claimed complete.

The cleanup removes the obsolete whole-body solver, heuristic inertia and
legacy generation/comparison naming. The unchanged radius-fit equations now
belong to `structure/fallback/fits.rs`, with regressions for compression,
stripping, thermal trends and broad prescribed reference worlds. Retention
owns its own errors; shape never derives inertia from a display class.
`./scripts/check.sh` passes (471 simulation and 392 client tests), as do all
27 envelope-filtered fixtures and the saved 315-fallback audit (radii unchanged).
Fresh standard coverage remains **1,437/1,544 layered and 107 checked fallbacks**.
Logs: `data/tmp/p3-cleanup-{check,envelope,audit,coverage}.log`.

1. H/He is an ordinary exchangeable gas, with no amount threshold. A body enters
   the **envelope route** only when the thin column refuses its gas: H/He alone above the
   coupling's 1% gas-mass mask (H/He never condenses, so this is checked first),
   or a coupled refusal for gas mass, dense gas or mass above 20 Earth masses on
   an H/He-bearing body. The new coupled envelope route is then attempted;
   envelope-route totals include both its successes and explicit refusals.
2. Protected secondaries (90% of CO₂/N₂/CH₄/O₂/Ar source, capped by retained
   supply) stay entrained in the interior. The coupled solver includes that
   mass in every iteration, with H/He, **all** water and all remaining
   secondaries exchangeable, at the greenhouse surface temperature over the
   equilibrium temperature (floored at 20 K, as in generation). Water is not split: exposed water evaporates until
   the air is saturated, leaving an ocean or ice below, or all of it becomes
   steam over dry rock. Generation's 99.999% protection shields water from
   escape only, which has already been applied.
   The coupled call goes through the [blow-off wrapper](#unbound-column-blow-off),
   so a weakly bound column loses its lightest gases instead of failing. The
   report counts escaping planets, their species and escaped mass fraction.
3. Each iteration uses the dry or water protected-trace radial solver under
   the coupled load (gas pressure plus any frost deposit), with the same
   central-pressure hints and shared work ceiling. Protected mass contributes
   to volume, gravity and inertia throughout; no separate final re-solve is
   needed. Any frost sits on top: the surface is reported as `frost` and the TSV
   carries `deposit_m`.

The immutable inputs are generation's `Budget` and `PlanetEnvironment`. Only
the equilibrium temperature is read from the environment; legacy pressure,
atmosphere and reservoir assignments do not seed the new physical answer.
Generation's retained inventory and prior losses are authoritative, so retention
history is not run again. The result contains the region-owned `CoupledStructure`
(ordered interior layers, deposit, gas, atmosphere, boundary facts and model
identities), original source, total escape and additional blow-off separately.
The protected inventory is audit-only, already owned by the interior layers.
Source equals the returned retained regions plus total escape for every origin,
with relative 10⁻¹⁰ closure. Unsupported inputs return the original typed error,
never legacy geometry. Methane remains an ordinary secondary gas/entrained trace.

Seven entry-point tests cover dry/icy/ocean hosts, frozen deposits, evaporation
of all water into gas over dry rock, protected supply surviving blow-off, prior
losses, deterministic repeats, unchanged inputs, no-trace compatibility, exact
shared-work success/failure and explicit invalid/envelope refusals.

Step 7 replaces the provisional two-pass route, rather than just wrapping it.
The 20-thread coverage run still solves 1,017/1,032 non-envelope planets, with
all outcome/reason, surface and escaped-species labels and escaped fractions
unchanged. Including the previously omitted protected mass in the column's
gravity changes temperature by at most 0.067 K, pressure by 0.022 bar and radius
by about 0.0006% relative to the former join. This is a coupling correction, not
looser numerical tolerances. Report wall time was 13.9 s (previous run 15.5 s),
with 565,778,193 reported EOS evaluations instead of 818,072,711; the redundant
final interior solve is gone. Timing comparisons remain load-dependent and the
work counter still excludes discarded escape attempts.

Each planet reports only its **first** blocker. Water failures are binned by
condition (below 250 K, above 623 K, vapour, surface below 1 kPa, otherwise deep
or phase seam) rather than error variant, so each bin maps to one plan step.
A failed coupled solve does not report its warmed temperature, so its water
failures fall in the vapour, low-pressure or deep bins only. Bands are still by
equilibrium temperature; the TSV carries `surface_k` for solved bodies.
Fixing one blocker can expose another, including the dense-gas limit for bodies
with 0.1–1% H/He.

Formation gives H/He to nearly every body above about 0.1 Earth masses of
protected heavy material, and escape never removes it exactly. Treating any H/He
as an envelope therefore misclassified 472 stripped rock, metal and water worlds
with less than 0.01% H/He by mass.

P1 baseline after unbound blow-off and greenhouse refinement optimisation
(October 2026, 20 systems per component):
**1,017 of 1,032 non-envelope planets solve (99%)**, from 1,544 planets with
512 H/He-bearing envelopes. Bands by equilibrium temperature: 227/227 below
50 K, 411/419 at 50–250 K, 292/299 at 250–623 K, 87/87 above. Outer layers:
frost 422, ice 202, ocean 28, rock 365.

The refinement-only optimisation preserves the 0.5% upward scan, equations,
partition solver and radial solver. A paired 20-thread release coverage run on
the same 1,544 planets measured:

| Metric | Before | After |
| --- | --- | --- |
| Report wall time | 19.5 s | 18.2 s |
| Solved planet median | 152.27 ms | 126.90 ms |
| Solved planet p95 | 621.06 ms | 573.12 ms |
| Solved planet maximum | 9,992.71 ms | 9,813.92 ms |
| Ocean median | 2,814.54 ms | 2,752.72 ms |

All outcome/reason, surface and escaped-species labels matched, as did escaped
fractions. Maximum differences: temperature 1.21 × 10⁻⁹ K, surface pressure
3.51 × 10⁻¹¹ bar and relative radius 4.36 × 10⁻¹⁴. The TSV now preserves 17
significant digits for physical values so comparisons do not hide differences
through display rounding. These timings include thread contention, are not
uncontended per-body CPU timings, and vary with machine load. The unchanged
scan and repeated interior solves still dominate the remaining cost.

The subsequent interior-hint optimisation was measured separately with another
paired 20-thread run, retaining greenhouse refinement and excluding the reverted
gas-search caching experiment:

| Metric | Before hints | After hints |
| --- | --- | --- |
| Report wall time | 17.9 s | 15.5 s |
| Solved planet median | 129.51 ms | 109.73 ms |
| Solved planet p95 | 576.94 ms | 548.61 ms |
| Solved planet maximum | 7,089.56 ms | 6,903.73 ms |
| Ocean median | 3,052.32 ms | 2,223.82 ms |
| Reported radial EOS evaluations | 1,137,912,815 | 818,072,711 |

Every outcome/reason, surface and escaped-species label matched for all 1,544
planets; escaped fractions were identical. Maximum differences: temperature
1.07 × 10⁻⁹ K, pressure 3.62 × 10⁻¹¹ bar and relative radius 2.03 × 10⁻¹⁴.
Reported EOS work fell 28%, a timing-independent indication of reduced numerical
work. That counter covers successful reported structures, not all failed or
discarded escape attempts; timings still include contention/load variability.

- All 178 former weak-binding failures now solve by escape, 3 of them former
  envelopes that lose only hydrogen. Lost species: water and methane 130; water,
  nitrogen and methane 24; all secondaries 21. Escaped mass fraction: under
  0.1% median, 1.0% p95, 3.7% maximum.
- Remaining first blockers: dense gas 8, warming beyond the bounded column 7.
- Radius difference from legacy: 1.2% median, 9.6% p95, 12.7% maximum.
- Diagnostic solve cost on 20 threads: 156 ms median, 593 ms p95, 9.8 s
  maximum; report wall time 19.0 s.

Baseline after step 5, before blow-off: **839 of 1,029 (82%)**, 515 envelopes.
All 280 water-rich planets solve. Bands: 227/227 below 50 K, 398/419 at
50–250 K, 154/298 at 250–623 K, 60/85 above. Outer layers: frost 422, ice 202,
ocean 28, rock 187.

- First blockers then: weak binding 175, dense gas 8, warming beyond the
  bounded column 7. No deep-water or column-temperature first failures remain.
- Solved count increased by **91**, not by 91 plus the reclassified envelopes.
  Envelopes increased by 66 because extending warming past the old ceiling exposes
  density/binding failures on H/He-bearing bodies under the existing triage rule.
  Trace H/He alone is still not an envelope criterion.
- Radius difference from legacy: 1.9% median, 10.0% p95, 12.7% maximum.
  Surface pressure: 0.016 bar median, 37.9 bar p95, 379.2 bar maximum.
- Diagnostic solve cost on 20 threads: 156 ms median, 535 ms p95, 7.2 s maximum;
  full report wall time 17.3 s. Performance still needs attention before step 8.

Historical baseline before step 5 (October 2026, 20 systems per component, greenhouse surface
temperature, after P1 steps 2, 3, 4 and 6): 1,544 planets, 449 envelopes.
**748 of 1,095** non-envelope planets solve (68%): all 227 below 50 K, 391 of
429 at 50–250 K, 95 of 345 at 250–623 K and 35 of 94 above (bands by
equilibrium temperature). Outer layers are frost 422, ice 202, ocean 21 and
rock 103. First blockers among non-envelope planets: gas triage 15% (155 weak
binding, 7 dense gas), greenhouse runaway 15% (159), column above 1,000 K 1.4%,
deep water or phase seam 1.0%.

- **Step 2 moved coverage from 88% to 68%, by design.** Warm water worlds now
  run away instead of keeping an ocean at their equilibrium temperature.
  - **Runaway (159):** equilibrium 176–977 K, median 370 K; median water
    2.5×10⁻⁴ of the mass, about one Earth ocean. The coolest ones carry several
    % water or thick CO₂ frost that evaporates as they warm. Their end state is a
    hot, dense steam column this thin-column model cannot hold (P2).
  - **Oceans fell from 151 to 21.** They now sit at equilibrium 163–284 K,
    warmed to 274–313 K. Before, every ocean was an unwarmed body above 273 K.
  - **Warming** of solved bodies: 0.6 K at the median, 335 K at p95, 595 K at
    most (dry rock under steam, Venus-like).
  - **Weak binding rose from 109 to 155:** warmer columns have larger scale
    heights, so more small bodies fail the sonic-radius guard. These are small
    bodies (median 0.006 Earth masses) whose steam column would escape; the
    escape question from step 4 is still open.
- Envelopes went from 454 to 449: a few thin H/He columns are warmer and so less
  dense.
- Earlier, step 4 raised envelopes from 417 to 454 (steam added to thin H/He
  columns) and step 3 from 219 to 417 (cold bodies with 0.1–1% H/He now exceed
  the dense-gas limit).
- **Radius against legacy:** p50 2.4%, p95 10.2%, at most 12.7%. Icy bodies grow
  most: a Pluto-mass body with 58% water comes out about 12.6% larger, at about
  1,350 kg/m³ mean density, because ice Ih (about 934 kg/m³) is lighter than the
  legacy fit assumes. Massive icy bodies come out up to 7% smaller from deep-ice
  compression.
- Frost deposits are 0.8 m at the median, 216 m at p95 and under 1 km at most.
- A solve takes about 110–190 ms at the median on 20 threads (the warming scan
  adds partition work) and up to about 9 s. Solved surface pressures reach about
  10 bar at p95, 41 bar at most.

## Light envelopes and the giant transition

The following describes the existing **generation approximation**, not a dense
H/He radial solution. Checkpoint 4's opt-in coupling API above does not silently
substitute this approximation when its supported column domain is exceeded.

The input is retained H₂+He mass fraction `f`, total mass `M`, current bolometric
flux `F` and age, not class or surface pressure. Helium must be 15–35% of the
H/He mass and all H/He must be allocated to the gas reservoir. H/He planets require
0.1–4,000 total Earth masses; dry bodies allow 0.0001–4,000, subject to the separate
heavy-mass bound above. Pure H/He is supported; pure H or pure He is not.

The starting thickness is inspired by Lopez & Fortney (2014), equation 4:

```text
ΔR / R_earth = 2.06 M^-0.21 (f/0.05)^0.59 (F/F_earth)^0.044 (age/5 Gyr)^-0.18
```

The original low-mass envelope calculations are not a giant-planet prescription.
We bound its mass input to 1–20 Earth masses, fraction to at most 0.2, flux to
0.1–1,000 Earth fluxes and age to 0.1–10 Gyr, flagging any bound used. Age must
still be positive and finite; flux must be finite and nonnegative. Below the
published grid's minimum fraction (0.0001), the power law is deliberately
continued to zero for a continuous stripping limit, not a claim of fit accuracy.
Every envelope result carries an `EnvelopeSurrogate` caveat.

Our separate, tunable self-compressed giant surrogate is:

```text
R_g / R_earth = 11.2 (M/318)^(1/3)
               × [(1 + 318/95)/(1 + M/95)]^(1/3 + 0.04)
```

This tends from `M^(1/3)` to `M^-0.04` and normalises a 318-Earth-mass gas sphere
to 11.2 Earth radii. It is **not** a published giant evolution fit. It has no hot-
Jupiter inflation, explicit entropy/cooling integration or helium separation.
Set `R_g` no smaller than the heavy-material proxy. Limit the low-mass thickness
smoothly to its available radius using `d / [1+(d/c)^4]^(1/4)`, where `c=R_g-R_heavy`.
Zero capacity gives zero thickness. Blend from this radius to `R_g` across
`f=0.1–0.5` using clamped smoothstep `t²(3-2t)`. Thin-envelope mass/age/flux fits
therefore never directly set gas-giant radii.

These choices guarantee finite bounded radii and continuous transitions within
the domain, not uniquely correct structures. The reference radius is a one-bar
**proxy**, not the transit radius of the original low-mass model. No photospheric
radius correction is added, so this is not a reproduction of that published fit.

As a necessary supply check, bound that proxy with the thin-column relation
`M_column ≈ 4π R⁴ p/(GM)` at `p=100,000 Pa`. The implied upper support radius
limits excess radius with the same smooth limiter. This is not used to infer the
mass of the entire giant envelope. If even the heavy boundary cannot support
one bar, return its radius and condensed-boundary semantics with `SubBarEnvelope`.
Tiny H/He inventories therefore do not fabricate a one-bar reference level.
No giant envelope bottom, ocean bottom or atmospheric discontinuity is inferred.

## Constants, outputs and limits

`structure/parameters.rs` centralises empirical coefficients, model bounds and
engineering defaults separately from physical constants in `units.rs`. Earth
radius is 6,371,000 m and reference flux 1,361 W/m². The mass unit is derived from
the existing orbital `SUN_GM × EARTH_SOLAR / G`, using CODATA 2022 `G=6.67430e-11`.
This preserves the simulator's existing gravitational mass convention.

Outputs are retained kilograms, volume-equivalent metres, H/He mass fraction,
reference semantics, model caveats, `ρ=M/(4πR³/3)` and `g=GM/R²`. Escape is never
included in those quantities. Shape, spin and classification do not enter the
calculation. The solve is algebraic, bounded-cost and deterministic; there is
no iterative convergence failure or random fallback. Invalid/unsupported states
return typed errors. Inclusive mass-bound checks allow 16 machine epsilons for
summation roundoff only.

### Descriptive labels

Descriptive thresholds are centralised separately from structure inputs. Display
precedence uses retained mass fractions in this order:

1. H₂/He at least 50%: **Gas giant**.
2. H₂/He at least 10%: **Envelope world**.
3. H₂/He at least 0.1%: **Sub-Neptune**.
4. Water at least 25%: **Water-rich planet**.
5. Iron/nickel at least 50%: **Metal-rich planet**.
6. Otherwise: **Rocky planet**.

Equality takes the earlier rule. These are adjustable, disjoint display buckets,
not universal size definitions: conventional giant/sub-Neptune names can overlap
observationally. Water-rich does not mean an ocean, and envelope-world does not
assert an ice interior. No mass-based `Dwarf` class is generated or searchable:
dynamical orbital clearing is not established here. Changing these thresholds
changes descriptions/search matches only, never structure, spin, gas or optics.

Tests check pure and mixed material ordering, increasing compression, conserved
reservoirs/escape, mole/mass conversion, stripping and blend continuity, invalid
inputs, model warnings and deterministic finite outputs over a prescribed grid.
Broad checks allow 0.1 Earth radii around Earth, 0.06 around Mercury, 3–6 Earth
radii for a prescribed Neptune-like input, 8–12 for Saturn-like and 10–12 for
Jupiter-like. These are regression guardrails, **not measured error bars**.
The mixture/giant approximations have no established statistical uncertainty;
temperature, differentiation, entropy and phase errors can be substantial.
The deterministic calibration census and its limits are recorded below.

## Formation priors and retained structure

`generation/materials.rs` isolates tunable population heuristics. These are not
observationally fitted disc-accretion models. For proposed formation distance
`x=a/snow`, ice availability is `I=x⁴/(1+x⁴)` and feeding factor is
`(1+2I)/(1+(x/5)²)`. Draw a log-uniform heavy mass from 0.0022–12 Earth masses
times feeding, with a 0.0022 minimum. A compact retry multiplies feeding by 0.01
before that minimum. Water is `I × uniform(0.05,0.6)` of heavy mass; secondary
molecules together take a log-uniform 1e-7–0.003 share. The remainder is metal
and silicate, with metal taking 15–70% of rock. Secondary material is 98% split
between CO₂ and N₂ (5–90% CO₂ share), 1% CH₄ and 1% Ar. No free O₂ is drawn.

Heavy material starts in interior (rock/metal) or unresolved condensed volatiles
(water/secondary molecules). This is an initial accounting allocation, not a
solved phase state. H/He acquisition uses gas/heavy mass ratio
`onset × (M_heavy/5)^2.5 × log_uniform(1e-4,30) × min(M_host,1)/(1+x/20)`.
`onset=clamp((M_protected-0.1)/0.1,0,1)` smoothly excludes unsupported small-envelope
bodies. `M_protected` subtracts all accessible water/secondary material from heavy
mass, so subsequent escape cannot move a trace envelope below the 0.1 Earth-mass
domain. This is a formation prior, not a post-loss mass clamp. Gas is 75% H₂ and
25% He by mass, limited to 3,900 minus heavy mass in
Earth units, within the structure domain. Lower-mass hosts therefore acquire
less gas at equal core/seed. All allocations are scaled together if formation
mass exceeds 0.4% of current host mass. The recomputed sum is the source budget.

The formation mass sets conservative Hill spacing. Two algebraic inequality
solutions replace unbounded outward stepping; the host-mass cap keeps both
denominators positive. Resonances and companion bounds still constrain final
placement, and the 1,000 AU compact retry remains. The proposed formation location
is retained even when spacing moves the final orbit outward. Evolved hosts keep
their existing progenitor snow/inner scales rather than using present luminosity.

Retention shares the cached 32-phase all-star heating and activity/history-weighted
loss forcing. `system/forcing.rs` holds validated thermal/loss inputs, prepared
before retention or atmosphere partition: flux, loss forcing, age and the original
per-body seeded albedo. Its atmosphere-free environment adapter cannot import
previous pressure, partition or boundary geometry into a trial. Derived atmosphere
output is built separately from the same inputs.

For original gas mass `M_g`, solve
retained fraction `s` such that `s=1/[1+(25/λ)^4]`, where `λ=GM μ_H2/(R kT_exo)`
is evaluated at the **retained** mass and radius. Every trial now uses
`fallback::initial`, not `structure::derive` or a layered/EOS solve. The fallback uses
current flux and age, while `T_exo=max(1000 max(F_loss/1361,0.01)^0.2,2T_eq)`
and `T_eq=max(20,[0.7F/(4σ)]^0.25)` are fixed-albedo heuristic estimates.
These are not actual XUV spectra, Jeans escape rates or integrated evolution.
H₂ and He have the same retained fraction, deliberately deferring fractionation.
Rock/metal are fully retained. Accessible secondary loss is applied first, using
the fixed pre-loss source structure at the final irradiation and age (see below).
It is not iterated inside the H/He solve. All remaining heavy material enters
every H/He trial, including its protected and accessible volatile inventories.

Scalar bisection brackets `s` on [0,1], with at most 40 steps. Stop when absolute
survival residual or bracket width is ≤1e-8. This bounds gas-mass uncertainty to
1e-8 of original gas supply for a bracketed continuous root when the width
criterion is met. A residual stop only bounds the survival equation's residual,
not distance to a unique solution. It does not establish physical uncertainty.
The stripped endpoint is continuous in radius; bisection does not require a
monotonic radius law. No-envelope inputs bypass iteration. Trial structure is
explicitly fallback, with no fabricated layered refusal. Exhaustion returns an
explicit error; production asserts its draws converge rather than accepting the
last trial silently. No external H/He data is needed by retention.
Re-evaluation always starts from immutable source H/He, so loss is not compounded.
Mass, radius, inertia and descriptive classification are updated together; neighbour
stability is checked again with retained masses, without moving any orbits.
Work is bounded by 40 fallback evaluations, each using fits and fixed profile
quadrature, plus one pre-loss secondary reference and the no-envelope shortcut.
There is no adaptive interior integration or repeated stellar phase sampling.
Valid zero irradiation reaches the fit's documented low-flux bound rather than
being replaced with Earth flux; equilibrium zero remains separate from the
fallback's 20 K temperature floor.

Formation starts with a validated fallback result; final retention replaces it
and installs every live field from it together (see
[live projection](#live-projection)). Neither stage runs `structure::derive`.
Retained population values changed at this cutover; archived P2 census evidence
belongs to its original inputs.

## Secondary escape and reservoir partition

The accessible fractions are tunable outgassing/exposure priors: `a=1e-5` of
source water and `a=0.1` of each secondary molecular constituent. The rest stays
protected in `CondensedVolatiles`. No mantle release, ocean evaporation history,
photochemistry, oxygen production or exchange between molecular budgets is solved.
For source mass `S_i`, accessible survival `r` gives retained mass
`S_i [1-a_i(1-r)]`, escaped mass `S_i a_i(1-r)` and available gas supply
`retained_i - S_i(1-a_i)`. Cold trapping does not delete the protected supply.

`r=1/[1+(F_loss/(1361 × 10 × (v_escape/11200)^4))^1.5]` is the existing
shoreline-inspired trend, now used as a surviving **mass fraction**, not a random
probability that air exists. Escape speed uses fallback for immutable pre-loss source structure
at current flux/age. This fixed reference avoids a second nonlinear solve but
does not reproduce feedback from secondary loss, envelope shielding or differential
escape. It is not an integrated escape rate or a fitted universal shoreline.
Re-evaluation starts from source, never from already depleted retained material.

The [atmosphere model](atmospheres.md#inventory-backed-generation) partitions the
accessible retained supply using its prescribed temperature and saturation caps.
All retained H₂ and He remain in `Gas`; other molecules not in gas remain in
`CondensedVolatiles`, including newly cold-trapped material. This reservoir name
does not assert a resolved phase, ocean or exposed ice layer. Atomic reservoir
transfers preserve authoritative retained/escaped totals, so partition does not
change bulk mass. Generation partitions once, at the converged retention
fallback's fixed reference ([reference partition](#synchronous-reference-partition)),
without feeding back into escape. Sub-precision gas transfers are omitted, not
manufactured. Trace gas below the 0.1 Pa display cutoff remains in the result.

Boundary-column pressure is normalised to the finite spherical gas shell,
`p=M_gas g/(4πR² F)`, with the [profile mass factor and weak-binding fallback](atmospheres.md#pressure-and-cold-trap-solve).
One-bar envelopes instead use
the complete gas reservoir's molecular mixture at a fixed reference pressure;
that column is never inverted to infer total giant mass. Both obey partial-pressure
cold caps with explicit mass-to-mole conversion. The partition uses a fixed
64-step scalar bisection, not a deep atmosphere integration. Ideal gas and
well-mixed bulk gas remain prescriptions, especially unreliable for
dense secondary air and escape-dominated columns. Heavy vapour still uses the cold bulk
radius proxy, with no steam-envelope expansion or greenhouse feedback. These
limitations are explicit rather than silently imposing an arbitrary pressure cap.
Emitted columns are checked against saturation and one-bar supply with a 1e-9
relative roundoff allowance; failure is explicit rather than a fabricated column.

### Removed legacy boundary correction

The legacy `structure/boundary.rs` correction (a condensed-boundary column test
on the `structure::derive` radius, its 1%/1%/20 Earth-mass domain, the
`thin_boundary_column`, `extended_boundary_column`,
`boundary_reference_transition` and `deep_boundary_unresolved` labels, and its
one-bar handoff blend) was removed at the live cutover. Generated planets now
take their reference meaning from the active result: gas may use a one-bar
coordinate, while sub-bar or boundary columns reference the condensed top.
Neither H/He presence nor a one-bar coordinate erases the condensed boundary.
The geometry/optics adapter decides whether that top can be drawn through the
air or stays behind an unresolved opaque proxy. Background refinement may also
replace a fallback gas reference with a solved condensed boundary and its actual
pressure. History: [plan](../plans/planet-composition.md#p3-default-cutover-and-legacy-removal).

## Inertia and spin

Initial generated inertia factors come from the fallback's layered shell
integration, including the reference partition's atmosphere column, normalised
by the reference radius. They range from about 0.06 (centrally condensed
envelope worlds) to 0.37. The obsolete clamped structure heuristic has been
removed. The fallback integration is not a giant-interior prediction.
After adoption, inertia comes from the layered result's region integration.
The assigned rotation phase/period is preserved, with shape recomputed using
the same bounded flattening prescription, not a new angular-momentum or spin-
evolution solution.

For spins, set `e=clamp(f/0.5,0,1)`, `q=clamp((0.02-M)/0.02,0,1)` (Earth mass
units). The lock distance is `(0.3-0.2e)(1-q) M_host^(1/3)` AU. Within 2.5 times
that distance, bodies with `f<0.1` can receive the existing slow or 3:2 states.
Otherwise free periods are log-uniform between `10-e-6q` and `60-45e+340q`
hours. With probability `(0.75+0.1e)(1-q)`, tilt is Rayleigh with scale
`22-4e` degrees; otherwise isotropic. These are prescribed Solar-System-inspired
outcomes, not integrated tidal histories. Spin streams are per stable body ID,
independent of orbital/material/atmosphere draws. Free spins are limited using
the physical inertia factor and the existing 0.2 flattening cap. Locked/slow
states keep their assigned periods, with the existing shape cap as a safeguard.

`Planet::inertia_factor` is optional for legacy compatibility, transported early
on both generated and task-overlay paths. Values in the Darwin-Radau domain
`(0, 2/3]` override all class strings in `planet_shape`, so shape uses the same
factor that limited spin. Missing or malformed factors use a neutral response
of 0.33, never a display-class heuristic, including older records without composition.

## Transport and downstream consumers

`Planet::physical` carries stable material IDs with source, retained, escaped,
interior, condensed and whole-gas masses, plus reference structure and stable
approximation IDs. It is optional: absent means unknown, not zero inventory.
For generated planets it is the result's record verbatim; `physical::validate`
checks it, and every other live projection, against `Planet::initial_result`
using transport invariants only (see [live projection](#live-projection)). It
repeats no stellar heating, retention, structure or partition solve. A
pressure-only label edit is rejected because the atmosphere must pass the
result's optical adapter.

Generated systems and task overlays use one conversion. Their physical records
must match for the same body ID; task metadata never alters them. New reflectance
uses retained water/metal and H₂/He continuously, not classification. See the
[rendering guide](../client/rendering.md#planet-reflectance-and-phase) for this
unresolved appearance proxy. Old records keep isolated class-based reflectance
and shape compatibility; invalid new reflectance uses a neutral proxy. Geometry,
surface boundary semantics and atmospheric optics do not inspect class strings.
Search consumes descriptions only. The [typed inspector](../client/ui/interface.md#breadcrumbs-and-inspector)
and developer export share presentation fields, separate from these physical records.

Atmospheric visibility has its own
[explicit optical boundary contract](atmospheres.md#explicit-renderer-boundary-contract).
It declares a profile, lower proxy and operational cutoff rather than using
structural layer edges. The renderer builds it through the checked
unified-result adapter, so a generated planet's air uses the result's reference
radius and enclosed gravity, and live air that disagrees with the result is not
drawn.

## Calibration census and regression scope

Reproduce the diagnostic census and prescribed reference-world table from the
repository root (CPU tests, not graphical/performance benchmarks):

```sh
cargo test -p space-simulation deterministic_population_census -- --nocapture
cargo test -p space-simulation prescribed_reference_worlds -- --nocapture
```

The census uses `composition-calibration:{component:?}:{index}` for indices
0 through 255 in each of all seven galactic components: **1,792 systems and
19,440 planets**. Components are sampled equally, **not** weighted by the galaxy's
stellar counts, survey detectability or observed exoplanet occurrence. These are
internal diagnostic seeds, not navigable galactic catalogue IDs. Generated ages,
stellar hierarchies and host masses are retained, not redrawn to improve a result.
Percentiles use the sorted element at `floor((N-1)q)`, without interpolation.

### Prescribed reference structures

At 4.5 Gyr, use the named *prescribed* budgets with local bolometric fluxes of
9,087, 1,361, 1,361, 1.5, 15 and 50 W/m² respectively. The dry Earth mixture is
32.5% metal and 67.5% silicate; Mercury uses 70%/30%. Water-rich replaces 50% of
the Earth mixture with H₂O. Neptune-like uses 60% H₂O and 15% H₂/He, while
Saturn-/Jupiter-like use 75%/90% H₂/He and no water. Remaining heavy mass uses
the Earth metal/silicate proportions. H₂/He is always 75%/25% by mass.

| Prescribed input | Mass (Earth) | Radius (Earth) | Density (kg/m³) | Accepted radius range (Earth) |
| --- | ---: | ---: | ---: | --- |
| Mercury-like | 0.0553 | 0.352 | 6,996 | 0.323–0.443 |
| Earth-like | 1 | 0.960 | 6,237 | 0.9–1.1 |
| Water-rich | 1 | 1.222 | 3,023 | 1.1–1.6 |
| Neptune-like | 17 | 4.758 | 870 | 3–6 |
| Saturn-like | 95 | 10.005 | 523 | 8–12 |
| Jupiter-like | 318 | 11.200 | 1,248 | 10–12 |

The tolerances allow unresolved layering, thermal state, composition and radius
reference errors, not statistical uncertainty. Tests additionally bound density
broadly and check increasing water radius, condensed compression, stripping,
blend continuity, thermal trends and giant-radius turnover. The dry Earth radius
is about 4% low; the Neptune-like prescribed radius is about 23% larger than
Neptune's nominal radius. These are substantial biases, not precision calibration.
The last three inputs do not reconstruct actual giant compositions or formation
histories. Do not tune mixtures to force exact Solar System matches.

### Population summary

The numerical tables below are the calibration snapshot from before the
extended-boundary correction. Low-inventory extended-atmosphere bodies now keep
their condensed radius instead of falling back to a one-bar radius, so radius,
density, reference and column distributions can differ. The commands above
produce the current census; the retained inventories and description thresholds
are unchanged by that correction.

| Quantity | Minimum | 5th percentile | Median | 95th percentile | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Mass (Earth) | 0.002199 | 0.002200 | 0.1282 | 11.303 | 2,854.901 |
| Radius (Earth) | 0.1231 | 0.1518 | 0.5349 | 3.8537 | 11.4125 |
| Density (kg/m³) | 249 | 723 | 4,700 | 7,055 | 12,018 |
| Water (retained mass %) | <0.000001 | 0.0001 | 3.692 | 50.646 | 59.986 |
| Metal (retained mass %) | 0.054 | 11.678 | 34.207 | 63.925 | 69.962 |
| H₂/He (retained mass %) | 0 | 0 | <0.0001 | 10.084 | 99.598 |
| Escaped/source mass (%) | <0.000001 | <0.000001 | 0.0008 | 4.836 | 91.868 |

| Description | Planets | Share | Median radius (Earth) |
| --- | ---: | ---: | ---: |
| Rocky | 8,482 | 43.63% | 0.427 |
| Metal-rich | 3,851 | 19.81% | 0.399 |
| Water-rich | 3,971 | 20.43% | 0.453 |
| Sub-Neptune | 2,158 | 11.10% | 2.109 |
| Envelope world | 467 | 2.40% | 4.620 |
| Gas giant | 511 | 2.63% | 9.653 |

Mass-bin counts for edges `[0.01,0.1,1,10,100,1000]` Earth masses are
`[3958,5208,4903,4236,923,204,8]`; radius-bin counts for
`[0.2,0.5,1,2,4,8,12]` Earth radii are `[2941,6289,4494,3458,1326,521,411,0]`.
Bins include their lower edge, with unbounded first/last bins. All description
populations appear, with no empty intermediate mass/radius bin at this resolution.
This is not a measurement of the exoplanet radius valley or observational completeness.

The census prints full per-component, age, host-mass, multiplicity and class
quantiles, fractions, caveat frequencies and histograms. Selected strata:

| Stratum | Planets | Median mass (Earth) | Median radius (Earth) | Radius p95 (Earth) |
| --- | ---: | ---: | ---: | ---: |
| Host <0.5 solar masses | 14,944 | 0.146 | 0.558 | 3.580 |
| Host 0.5–1.5 solar masses | 4,293 | 0.082 | 0.461 | 4.285 |
| Host ≥1.5 solar masses | 203 | 0.214 | 0.657 | 5.133 |
| Age <0.2 Gyr | 3,036 | 0.154 | 0.556 | 4.816 |
| Age 0.2–5 Gyr | 2,423 | 0.116 | 0.519 | 4.147 |
| Age ≥5 Gyr | 13,981 | 0.125 | 0.534 | 3.456 |
| Single-star system | 14,097 | 0.123 | 0.530 | 3.832 |
| Binary system | 3,978 | 0.122 | 0.524 | 3.658 |
| Triple system | 926 | 0.175 | 0.579 | 4.030 |
| Quadruple system | 439 | 0.247 | 0.662 | 4.564 |

System multiplicities are 1,324 singles, 368 binaries, 74 triples and 26 quadruples,
including systems with no surviving planet slots. Host mass means summed stellar
mass for a circumbinary host. These strata are correlated: high host masses are
sparsely sampled, and young stars mostly come from arms. Their differences alone
do not establish causal age, metallicity or multiplicity trends. A separate 800-case
formation/retention grid spans hosts 0.08–20 solar masses, formation distances
0.01–100 snow-line units, ages 0.001–13 Gyr, fluxes 0–1e8 W/m² and loss forcing
0–1e10 W/m². It checks the supported heuristic domain, conservation, bounded root
residuals and non-compounded escape, not self-consistent stellar evolution at all
those artificial age/mass combinations.

### Outlier review and known population artefacts

- **Lower-mass pile-up:** 1,100 planets (5.66%) have source mass at the imposed
  0.0022-Earth floor. Low feeding and compact retries create this artificial
  population edge; accessible escape can lower retained mass slightly below it.
  There are no sampled source masses at the 3,900-Earth cap. The floor and cap
  are declared formation priors, not physical discoveries. The diagnostic flags
  a floor share above 25%; it does not hide the current pile-up with random scatter.
- **Giant turnover:** the largest radius is 11.4125 Earth at 781.7 Earth masses
  (`composition-calibration:ThinDisc:209/4`). The concentration near a Jupiter-sized
  radius and high-mass density rise follow the smooth self-compression surrogate,
  not a class-based clamp. No hot-Jupiter inflation or true brown-dwarf cooling
  is modelled.
- **Dense boundary worlds:** the densest planet is now 12,018 kg/m³, a
  10.68-Earth-mass, 68.5%-metal body with 0.098% H₂/He and a 1.70-Earth-radius
  condensed boundary (`composition-calibration:Nucleus:33#B/4`). Its density uses
  the cold compressed proxy without the gas column's volume, unlike its former
  one-bar reference. Previously the densest was a 2,854.9-Earth-mass giant at
  11,285 kg/m³ (`composition-calibration:Arms:173/8`), which is unchanged.
- **Low-density tail:** the minimum is 249 kg/m³ at 2.32 Earth masses, 3.72 Earth
  radii and 27.2% H₂/He (`composition-calibration:ThinDisc:133#AB/5`, age 0.417 Gyr).
  Such a low-mass, substantial-envelope case is explicitly an envelope surrogate,
  not a validated low-density-world prediction. It remains finite and passes
  one-bar supply, optical conversion and navigation-clearance checks.
- **Historical atmospheric reliability (before spherical profiles):** these
  figures describe the earlier constant-gravity census, not current output.
  3,183 planets (16.37%) have absent/trace displayed
  air, while 7,257 (37.33% of all planets, 44.64% of displayed atmospheres) have
  extended constant-gravity columns. These frequently involve small low-gravity
  bodies or light gas. They are accepted *flagged approximations*, not evidence
  that escape, density profiles or drag-clearance heights are accurate there.
  The largest optical extent is 3.10 reference radii at only 0.207 Pa
  (`composition-calibration:Bulge:103#B/0`), a heated small body. It is explicitly
  extended, well outside the reliable constant-gravity regime; finite geometry
  must not be mistaken for a bound hydrostatic atmosphere. The largest boundary
  pressure is about 119,000 bar (11.9 GPa) under a 0.95% H₂/He envelope on a
  9.73-Earth-mass body (`composition-calibration:ThinDisc:21/1`). It is within the
  declared thin-boundary domain (extent 0.048 radii), and such pressures are
  plausible orders of magnitude beneath percent-level envelopes. Its ideal-gas
  density and dilute optics are certainly not valid there: this is a weight-of-gas
  estimate, not a dense-gas EOS, supercritical-phase or interior solution. No
  pressure cap removes its boundary.
- **Outside-fit usage:** 8,607 of 10,274 H₂/He-bearing structures bound at least
  one thin-envelope fit input; 96 use the high-heavy-mass continuation. All
  sampled bodies contain some water and secondary molecules, so all carry the
  cold-water and trace-volatile caveats, even when they are labelled rocky.
  No generated label establishes a dry exposed surface. The model is intentionally
  approximate for these populations, not validated against an occurrence catalogue.

No formation or structure coefficients were retuned for this census. Its review
does not justify an observational population fit, nor hiding the known artefacts.
The actual defect found was an overly strict *relative-to-loss* transport check:
a retained reservoir split moved a tiny escaped remainder by source-scale roundoff.
The species-local subtraction allowance above fixes that case without changing
generated masses, reservoirs, radii or descriptions.

Checkpoint 5b's boundary correction was rerun over the same 19,440 planets.
2,955 use a supported `thin_boundary_column`, 763 the flagged
`boundary_reference_transition` and 16,485 retain `deep_boundary_unresolved`
references (including dry and water-bearing cold proxies outside the domain, not
only giants). Supported boundaries occur in rocky, metal-rich and sub-Neptune
descriptions; no water-rich, envelope-world or giant planet qualifies. Masses,
inventories, escape and descriptions are unchanged. Extended columns fall from
7,275 at checkpoint 5 to 7,257, and the maximum extent/radius is unchanged.
Corrected radii move about 110 planets from the 2–4 into the 1–2 Earth-radius bin.

Every census planet passes transport reconstruction, species conservation,
spin/shape bounds, optical conversion (when air is present), and finite ordered
navigation bounds. Separate tests compare complete physical wire records after
reverse-order and four-thread regeneration, and search tests compare new-class
results under different thread counts/work slices and fresh per-search caches.
Existing normal/overlay parity, cold-partition, clearance/route and typed-inspector
regressions remain part of `./scripts/check.sh`. Domain-edge tests check explicit
acceptance/refusal, rather than substituting fallback worlds. No convergence
failure was observed; the census maximum was 27 of the allowed 40 bisection steps.
These checks supplement, rather than replace, final user visual review.

## Sources

- [Seager et al. (2007), Mass-Radius Relationships for Solid Exoplanets](https://arxiv.org/abs/0707.2895),
  ApJ 669, 1279, DOI 10.1086/521346. Pure compressed-material fits only; our
  mixture and high-mass continuation are not their layered numerical solver.
- [Lopez & Fortney (2014), Understanding the Mass-Radius Relation for Sub-Neptunes](https://arxiv.org/abs/1311.0329),
  ApJ 792, 1, DOI 10.1088/0004-637X/792/1/1. Low-mass H/He thickness scaling only;
  our limiting, reference convention and giant surrogate are separate heuristics.
