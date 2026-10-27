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
Generation still uses the approximate reservoir/radius model;
no planets, snapshots, inspector fields or rendering use the new solver yet.
The separate [water-shell solver](#headless-restricted-water-shell-solver) uses
direct [IAPWS water physics](water-physics.md) and an explicitly approximate
effective deep-ice compression model. The mandatory Earth-mass, 30%-water fixture
solves at both 250 K and 300 K. Cold deep ice is explicitly approximate rather
than resolved into individual polymorphs. The standalone
Lai ice-VII branch remains separate, not used by the radial solver.
Opt-in [condensed-boundary/atmosphere coupling](#headless-atmosphere-and-condensed-boundary-coupling)
is implemented without switching generation or consumers to the radial models.
[Protected secondary traces in dry hosts](#headless-protected-traces-in-dry-hosts)
and [water-bearing hosts](#headless-protected-traces-in-water-bearing-hosts)
are also headless and opt-in.

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
`atmosphere/condensation.rs`. Only cold refractory and water EOS currently live
under `materials`. Methane is already drawn by formation and can enter the gas
budget; neither methane liquids/lakes nor condensed CO₂/supercritical CO₂ are
resolved by the new radial/coupled path. A cold-trap cap is not a phase diagram.
Methane is treated as an ordinary gas; dedicated methane phase work is parked on
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
central-pressure bracketing/bisection. Steps end exactly at material mass
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
checkpoint 4 as an opt-in API. **Generation, transport, inspector, clearance and
rendering still use their previous paths.** No camera restriction is introduced.
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
  top takes the liquid-side limit. Protected water is supported; protected (non-exchangeable) secondary condensates are
  refused here and handled by the protected-trace host solvers instead.

### Greenhouse surface temperature

`Temperature::Greenhouse { equilibrium_k }` replaces the imposed temperature
with the [grey greenhouse estimate](atmospheres.md#grey-greenhouse-estimate).
The isothermal column and the condensed water both take the warmed surface
temperature `T_s`.

- **Solved on the coupled column.** At each coupling geometry `T_s` balances
  `T_s = f(T_s)`, where `f` warms `T_eq` by the optical depth of the gas
  partitioned at `T_s`. Warming raises the caps, so frost and water evaporate,
  which warms further.
- **The coolest balance.** `f` never falls as `T` rises, so a scan up from
  `T_eq` in 0.5% steps finds the first crossing, then bisects it to full
  precision. This is the balance a planet warming up from equilibrium would
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

At most **32 coupling iterations**, each with two 64-step partition evaluations
and finite-column pressure bisections, are permitted. The aggregate ceiling is
**16,000,000 radial EOS evaluations**, including nested work and retries. Each
radial call retains its existing 2,000,000 maximum and other ceilings. Remaining
aggregate budget limits each subsequent call; a failed call terminates the
request, never retries with reset work. Controls can lower ceilings, not raise
them. Nonconvergence is explicit.

```sh
cargo test -p space-simulation system::structure::coupled -- --nocapture
```

Nineteen tests cover thin nitrogen, cold/warm 30%-water bodies, protected water, trace
and absent gas, mixed N₂/CO₂, thin H/He, N₂ frost on rock, CO₂ frost loading a
warmer water host, a 30 K icy body with N₂ frost on airless ice, an ocean under its own saturated steam and trace water fully evaporated, an
Earth twin warmed from frozen to a liquid ocean, unwarmed transparent and absent
air, a steam runaway refused, bounded hot steam, lightest-first blow-off, sub-ulp vapour, stripping, deterministic repeats,
refinement, exact aggregate-work success/failure and unsupported inputs/regimes.
Independent radial quadrature closes shell mass and inertia to relative 10⁻⁷
and hydrostatic pressure drop to 10⁻⁹. Fourfold refinement changes radius,
pressure and water allocation by less than relative 10⁻⁷. These are numerical
targets, not scientific error bars.

The nitrogen fixture (0.325 metal, 0.674999 silicate and 0.000001 nitrogen Earth
masses at 250 K) takes three iterations to reach **6,113.588 km** and
**134,987.088 Pa**, using 1,002,398 radial EOS evaluations. Replacing 0.3 Earth
masses of silicate with exchangeable water gives **7,179.896 km / 70,950.716 Pa**
at 250 K and **7,170.596 km / 73,548.551 Pa** at 300 K. Both take three iterations,
using 1,085,619 and 2,793,870 radial evaluations respectively. These are prescribed
test compositions, not reconstructions of observed planets.

The cold fixture exposed a round-trip floating-point error: `(P/scale)*scale`
could lie one ulp beyond an outward EOS branch. The shared kernel now recognises
**exact equality** to a prescribed event's dimensionless coordinate and evaluates
the original endpoint pressure. No tolerance band, pressure offset, density
blend or EOS-domain extension is used.

## Prototype coverage report

An opt-in report measures how much of the generated population the
rock-and-water prototype path solves, so each P1 step of the
[plan](../plans/planet-composition.md) can be measured against it.

```sh
cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture
```

`NS_COVERAGE_SYSTEMS` sets systems per galactic component (default 20, using the
census keys `composition-calibration:{component:?}:{index}`).
`NS_COVERAGE_THREADS=1` gives uncontended per-planet timings. The summary is
printed and written to `data/tmp/composition-coverage.md`, with one row per
planet in `data/tmp/composition-coverage.tsv` for diffing between steps.

The route is provisional and test-only, until the single entry point exists:

1. H/He is an ordinary exchangeable gas, with no amount threshold. A body is an
   **envelope** only when the thin column refuses its gas: H/He alone above the
   coupling's 1% gas-mass mask (H/He never condenses, so this is checked first),
   or a coupled refusal for gas mass, dense gas or mass above 20 Earth masses on
   an H/He-bearing body.
2. Protected secondaries (90% of CO₂/N₂/CH₄/O₂/Ar source) are set aside. The
   coupled solver runs on the rest, with H/He, **all** water and all remaining
   secondaries exchangeable, at the greenhouse surface temperature over the
   equilibrium temperature (floored at 20 K, as in generation). Water is not split: exposed water evaporates until
   the air is saturated, leaving an ocean or ice below, or all of it becomes
   steam over dry rock. Generation's 99.999% protection shields water from
   escape only, which has already been applied.
   The coupled call goes through the [blow-off wrapper](#unbound-column-blow-off),
   so a weakly bound column loses its lightest gases instead of failing. The
   report counts escaping planets, their species and escaped mass fraction.
3. The resulting condensed host is re-solved with the protected secondaries
   entrained at the coupled load (gas pressure plus any frost deposit). The
   column's gravity ignores that protected mass, at most 0.3% of the body. Any
   frost sits on top: its thickness is added to the radius, the surface is
   reported as `frost` and the TSV carries `deposit_m`.

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

Current baseline after unbound blow-off (October 2026, 20 systems per component):
**1,017 of 1,032 non-envelope planets solve (99%)**, from 1,544 planets with
512 H/He-bearing envelopes. Bands by equilibrium temperature: 227/227 below
50 K, 411/419 at 50–250 K, 292/299 at 250–623 K, 87/87 above. Outer layers:
frost 422, ice 202, ocean 28, rock 365.

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

Final structure shares the atmosphere model's cached 32-phase all-star heating
and activity/history-weighted loss forcing. For original gas mass `M_g`, solve
retained fraction `s` such that `s=1/[1+(25/λ)^4]`, where `λ=GM μ_H2/(R kT_exo)`
is evaluated at the **retained** mass and radius. The structure solve uses
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
monotonic radius law. No-envelope inputs bypass iteration. Unsupported structure
or exhaustion returns an explicit error; production asserts its supported draws
cannot reach that failure, rather than substituting an invented fallback world.
Re-evaluation always starts from immutable source H/He, so loss is not compounded.
Mass, radius, inertia and descriptive classification are updated together; neighbour
stability is checked again with retained masses, without moving any orbits.
The cost is bounded algebraic structure work per bisection step, not numerical
interior integration or repeated stellar phase sampling.

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
probability that air exists. Escape speed uses immutable pre-loss source structure
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
change bulk mass. The environmental reference correction below updates radius,
inertia and spin together, without feeding back into escape. Sub-precision gas transfers are omitted, not
manufactured. Trace gas below the 0.1 Pa display cutoff remains accounted for.

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

### Supported approximate condensed boundaries

After escape, `structure/boundary.rs` tests a column at the cold heavy-material
radius using the actual generated isothermal temperature and the same conserved
cold-trap solve. The condensed-boundary inventory domain requires all of:

- Gas reservoir at most **1% of retained mass**, limiting neglected self-gravity.
- Water at most **1% of retained mass**, limiting use of an unresolved cold-water
  radius in place of a thermal, phase-dependent hydrosphere.
- Heavy material at most **20 Earth masses**, within the direct compressed fit.

These are declared engineering validity bounds, not observed divisions between
planet classes. No pressure or atmospheric-height threshold removes ground. Dense columns still use
unvalidated ideal-gas/dilute optics and must not be mistaken for solved climates.
Within the domain, the reference radius becomes the approximate condensed boundary,
pressure is normalised to the partitioned gas above it, and the column covers the
height to one bar plus the upper atmosphere. Even trace water keeps phase/ground
semantics unresolved. Dry inputs have nominal ground, not a solved geology.
Airless/trace inventory remains conserved even when no visible column is emitted.

The column is marked `thin_boundary_column` only when pressure-aware optical
extent is at most **0.1 boundary radii**, including the height to one bar. A
larger extent instead carries `extended_boundary_column` and informational
`Extended` geometry. It retains the approximate condensed radius,
inventory-normalised boundary pressure and actual gas reservoir. Supported columns
use weakening spherical gravity; cases outside the profile's sonic-radius guard
retain an explicitly unresolved exponential fallback. Extent alone is not evidence
that the condensed body is absent. Geometry, atmosphere,
optics and clearance all use that same declared reference. Neither case solves
greenhouse heating, atmospheric escape or water phases.

Let `s` be the maximum of the three inventory ratios to these domain bounds. At `s<=1`,
use the boundary column. At `s>=2`, retain the original structural reference and
mark `deep_boundary_unresolved`. Obviously excessive H₂/He, water or heavy mass
bypasses the candidate solve entirely: do not test a thin column deep inside a
giant. Atmospheric extent is not part of this handoff. For original one-bar structures with `1<s<2`, a flagged engineering handoff
uses `t=s-1`, `w=t²(3-2t)`: blend the candidate's **one-bar radius**
`R_boundary + h(ln(p_boundary / 1 bar))` using the selected hydrostatic profile,
towards the original one-bar proxy,
and blend per-species gas allocations towards the one-bar partition. Transfers
remain within conserved reservoirs; both endpoint mixtures obey the one-bar
saturation caps. This compares like pressure levels rather than blending a
boundary and a one-bar radius as if they were the same quantity. Equivalent
one-bar radius and gas allocations are continuous across both handoff endpoints.
The stored radius changes its meaning at the support boundary, so its numerical
value alone is not a continuity diagnostic. Recomputed gravity, scale height and
inertia remain reference-level approximations, not a variable-gravity interior.

Non-envelope cold proxies that lack a supported boundary retain their original
unresolved reference, rather than inventing a one-bar level or integrating deep
into a water-rich body. This exception includes tenuous extended columns with
less than one bar of gas. Their pressure is at a cold proxy, not a located ground.
The inspector explicitly marks the unresolved boundary/model limits.

This correction does **not** rerun escape against a newly shrunken reference radius.
The original formation/escape radius is a fixed prescribed surrogate; coupling a
thermal atmosphere to evolution is deferred. Masses, formation inventories, orbital
spacing, retained/escaped totals and class labels therefore do not change. Final
inertia and deterministic spin are rebuilt at the corrected geometric reference.
Partition work remains bounded: at most two 64-step cold-trap solves, after the
unchanged at-most-40-step retention solve. Geometry, gas optics and flight clearance
all receive that same final radius and column, with no class-based overrides.

## Inertia and spin

The structure model adds an explicitly heuristic inertia factor for Darwin-Radau:
`k=clamp((1-f) k_heavy (R_heavy/R)^2 + 0.25f,0.12,0.36)`, where
`k_heavy=0.36-0.08 × metal/heavy mass` and `f` is retained H/He fraction.
The radius ratio represents a compact heavy component under a larger envelope;
the bounds avoid claiming resolved central concentration. This is not a layered
moment-of-inertia integration or a prediction of giant interiors.

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
on both generated and task-overlay paths. Valid values override all class strings
in `planet_shape`. Only records without composition may use the isolated legacy
class fallback; malformed new records use a neutral factor, never their label.

## Transport and downstream consumers

`Planet::physical` carries stable material IDs with source, retained, escaped,
interior, condensed and whole-gas masses, plus reference structure and stable
approximation IDs. It is optional: absent means unknown, not zero inventory.
`system/physical` reconstructs validated inventories/budgets, re-derives structure
and boundary partition at transported irradiation/age/column temperature and checks
all duplicated facts, radius semantics
and caveats (1e-10 relative roundoff tolerance). Escaped-mass comparisons also
allow eight machine epsilons of that constituent's source mass: summing split
reservoirs then subtracting from a much larger source can move a tiny loss by
one ulp. This allowance is species-local, never scaled to total planet mass;
reservoir budget validation still forbids overspending, and negative/nonfinite
escaped masses are refused. Unknown/duplicate/missing materials,
nonfinite values and inconsistent records are refused explicitly. Partition checks
also allow species-local source-scale roundoff after reservoir subtraction, while
the atmospheric mixture must match the actual transported gas reservoir. Pressure,
temperature and reference level are checked against that inventory and geometry,
so a pressure-only label edit is rejected. This bounded check does not repeat
stellar heating or retention iteration.

Generated systems and task overlays use one conversion. Their physical records
must match for the same body ID; task metadata never alters them. New reflectance
uses retained water/metal and H₂/He continuously, not classification. See the
[rendering guide](../client/rendering.md#planet-reflectance-and-phase) for this
unresolved appearance proxy. Old records keep isolated class-based reflectance
and shape compatibility; invalid new reflectance uses a neutral proxy. Geometry,
surface boundary semantics and atmospheric optics do not inspect class strings.
Search consumes descriptions only. The [typed inspector](../client/ui/interface.md#breadcrumbs-and-inspector)
and developer export share presentation fields, separate from these physical records.

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
