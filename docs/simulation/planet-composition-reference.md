# Planet composition · scientific reference

This is supporting evidence and specification, **not another roadmap**. The
[active plan](../plans/planet-composition.md) owns status, work sequencing, blockers
and exit gates. The [implemented-model guide](planet-composition.md) owns runtime
APIs, physical domains and measured numerical results.

This reference preserves generated-input bounds, selected/candidate prescriptions,
source pins and fixed acceptance requests. Equations and offline audits do not
establish complete physical coverage. Cold dense H/He, secondary solid/deep/hot
routes, generated partition and thermal closure remain unresolved where stated.
No generation-domain reduction or runtime data import is implied.

## Generated inputs: measurements versus bounds

Reproduce from the repository root:

```sh
cargo test -p space-simulation deterministic_population_census -- --nocapture > data/composition-coverage-5.txt 2>&1
```

The calibration uses `composition-calibration:{component:?}:{index}`, indices
0–255 over seven components, including companion hosts. The added `replacement
coverage` block reports min/p05/p50/p95/max and stable IDs of both extrema for
each input/material fraction. Quantiles use the existing floor-index convention.
Protected fractions mean source mass times unexposed fraction divided by retained
total mass: supply constraints, not measured phases.

This run has 1,792 systems and 19,440 planets. All retain protected non-water
secondary supply; naturally drawn O₂ is zero. Rounded **sampled extrema**:

| Quantity | Minimum | Maximum |
| --- | ---: | ---: |
| Retained mass, Earth masses | 0.00219949 | 2,854.90 |
| Retained heavy mass, Earth masses | 0.00219949 | 29.8744 |
| Equilibrium temperature, K | 0.604492 | 1,658.73 |
| Current column-temperature prescription, K | 20 | 1,658.73 |
| Age, Gyr | 0.000711521 | 12.9857 |
| Incident flux, W/m² | 5.83531e-8 | 2.81126e6 |
| Retained water mass fraction | 2.55521e-10 | 0.599857 |
| Retained hydrogen mass fraction | 0 | 0.746981 |
| Retained helium mass fraction | 0 | 0.248994 |
| Retained CO₂ mass fraction | 1.17787e-10 | 0.00256081 |
| Retained N₂ mass fraction | 2.84940e-10 | 0.00271588 |
| Retained CH₄ / Ar mass fraction, each | 7.90468e-12 | 2.99941e-5 |

Stress IDs: `composition-calibration:Arms:103/8` (heavy mass),
`composition-calibration:Arms:173/8` (total mass),
`composition-calibration:Bulge:223/0` (temperature),
`composition-calibration:Halo:3/9` (minimum irradiation/temperature),
`composition-calibration:NuclearDisc:12/5` (minimum CH₄/Ar fractions).
These are diagnostic IDs, not added catalogue objects. Regenerate with
`galactic_bodies` and the encoded component, not a default component.

Source-derived constraints, distinct from measurements:

- `generation/materials.rs`: pre-cap heavy mass is at least 0.0022 Earth masses
  and at most 36 (conservative bound from feeding ≤3). The host cap can scale
  everything down; the draw floor is not a retained floor. Source total is at
  most 3,900 before the host cap; retention cannot increase it. Test 3,900, not
  merely the sampled 2,855.
- Water ≤60% of heavy supply; metal is 15–70% of the non-water/non-secondary
  remainder. Secondary supply is `1e-7..0.003` of heavy supply: 98% split CO₂/N₂,
  1% CH₄, 1% Ar. Pure endpoints/O₂ test the API, not natural formation frequency.
- `generation/retention.rs`: protected source fractions are 0.99999 for water
  and 0.9 for secondary molecules. Only accessible supply escapes/exchanges with
  air. H/He share survival and preserve 75:25 source mass ratio when present;
  arbitrary API inventories still need variable helium fraction Y.
- `atmosphere/generation.rs`: albedo `0.1..0.6`,
  `T_eq=[F(1-A)/(4σ)]^(1/4)`, column T=`max(T_eq,20 K)`. Neither is climate,
  deep temperature or entropy. Sample maximum T/flux/age are **not global bounds**;
  the conservative source-derived envelope below covers evolved/companion hosts.
- Existing radial scope (20 Earth masses), dilute coupling (20–1,000 K, ≤1% gas,
  ideal base density ≤10 kg/m³) and liquid water at or above its saturation
  pressure miss parts of this population. Replacement central P/T must be solved, not inferred
  from legacy reference pressures.

### Source-derived forcing envelope

This audit covers both galactic/tiered generation and the still callable legacy
page-star generator, not merely the seven-component census:

- Galactic and tiered ages are in [0,13] Gyr (`galaxy::Component` histories).
  Page main-sequence ages use the thin-disc history, at most 10 Gyr. Page giants
  have progenitor mass ≥0.9 solar masses, giving age
  `≤1.1 × 10 × 0.9^(-2.5) <14.4 Gyr`. Page white dwarfs have progenitor ≥1
  and temperature ≥4,500 K: age `≤11 +(4500/9000)^(-1/0.35) <18.3 Gyr`.
  Companions share the primary's age. **Use [0,18.3] Gyr for replacement inputs.**
  A zero lower endpoint is intentional: the RNG can emit zero and spiral-arm
  histories start there. Any thermal age scale must handle age zero explicitly,
  not introduce another invalid-input hole or change the draw.
- At most four luminous stars exist. Main-sequence mass ≤100 solar masses gives
  T<100,000 K from the implemented luminosity/radius laws. Giant/supergiant
  branches top out at 25,000 K; cooled white dwarfs at 100,000 K. All builders
  satisfy `L/R²=(T/5772 K)^4` in solar units. Display spectral bands are not bounds.
- Heating floors **each** emitter distance at that star's radius. Therefore
  every sampled contribution is bounded by its surface flux. Positive averaging
  over 32 orbital samples cannot increase the sum. Using the actual solar
  luminosity/radius normalisation, not assuming perfect consistency with σ:
  `F ≤4 L_sun/(4π R_sun²) × (100000/5772)^4 =2.26815515763693e13 W/m²`.
  With minimum albedo 0.1 this implies `T_eq ≤97,400.433 K`; a rounded conservative
  column-input envelope is **20–100,000 K**. Neither number is a likely habitable
  surface temperature or a proposed temperature clamp.
- The activity proxy obeys `integrated ≤age` since `1+ln(x) ≤x` for x≥1.
  Its hot-star multiplier is ≤30, so for age ≤18.3 Gyr its history factor is
  ≤1,143.75. Thus `loss_flux ≤2.59420246154724e16 W/m²`. This is a bound on the
  code's uncalibrated escape proxy, not an observed XUV-flux prediction.
- Flux is nonnegative. A tighter strictly positive lower bound is unnecessary
  for thermal closure, which must accept zero flux and cold outer regions.

These deliberately loose algebraic ceilings include unrealistically close
configurations; they avoid requiring a new orbital-stability proof. They are
**not evidence that dense molecular EOSs cover all the resulting states**.
The replacement still needs a hot/ionised outer route or evidence that surviving
material cannot reach it. Do not cap temperatures or delete hot planets to hide
that obligation. Regressions exercise the four-source distance-floor extremum
and the legacy page-age endpoints without altering any generation prescription:

```sh
cargo test -p space-simulation source_surface_flux_bounds -- --nocapture
cargo test -p space-simulation page_age_endpoints
```

## Material coverage matrix

Each capability needs model ID, SI units, validity predicate, approximation notes
and thermal requirements. Rectangular grids/library limits are not validity masks.

| Constituent | Route | Remaining specification |
| --- | --- | --- |
| Metal | Retain Seager 2007 Fe cold proxy, `ρ=ρ₀+cPⁿ`, 0–10¹³ Pa | Hot/deeper density and solidity route. Cold density is not solid/melt detection. |
| Silicate | Retain Seager 2007 MgSiO₃ cold proxy and its mask | Same thermal caveat. Separate core/mantle is prescribed, not computed differentiation. |
| Water | Retain bounded IAPWS/Ih/effective deep ice; revised AQUA 2026 broad-fluid candidate | Masked joins/thermal zeros. AQUA phase 5 conflates supercritical/superionic; phase 0 is extrapolated ice. |
| Hydrogen | CMS19 pure-H density/entropy candidate, original TP tables below | Cold dense solid/quantum mask and dilute join. Never use effective-H mixture table as pure H. |
| Helium | CMS19 pure-He candidate | Solid/quantum mask. Neutral helium is atomic; no automatic core or helium rain. |
| Nitrogen | Select Span 2000 pure-fluid equation, CoolProp v8.0.0 | Primary ceiling 1,000 K/2.2 GPa, not JSON's 2,000 K. Solid/deep and mixture routes remain open. |
| Carbon dioxide | Select Span–Wagner 1996 pure-fluid equation, CoolProp v8.0.0 | Primary ceiling 1,100 K/800 MPa, not JSON's 2,000 K. Need solid/deep route. |
| Methane | Select Setzmann–Wagner 1991 pure-fluid equation, CoolProp v8.0.0 | Melting-line to 625 K/1 GPa fluid domain. Need solid/sublimation/deep route; exposed liquid fixture is not a Titan reconstruction. |
| Oxygen | Select Schmidt–Wagner 1985 pure-fluid equation, tabulated in Stewart et al. 1991 | Primary ceiling 300 K/80 MPa. Resolve critical and freezing edge masks; preserve manual O₂ supply. |
| Argon | Select Tegeler 1999 pure-fluid equation, CoolProp v8.0.0 | Primary ceiling 700 K/1 GPa, not JSON's 2,000 K. Need solid/deep route; atomic gas, no fictitious pure trace shell. |

Mixtures require separate capabilities:

1. Dilute ideal mole mixing/partial-pressure saturation over nearly pure
   condensates can be a bounded approximation; fix its error/composition/density
   mask first. Pure critical points cannot decide mixture phase splits.
2. Dense H/He candidate: CMS19 pure endpoints plus Howard–Guillot 2023 variable-Y
   excess-volume/entropy terms. Require pure limits, Y=0.25 and comparison with
   CD21 Y=0.275. Never correct an already nonideal CD21 mixture twice or separately
   count metals already absorbed in effective Y=0.292/0.297. Equations/units/masks
   are pinned below; full physical masks and joins remain open.
3. Protected secondary traces use the selected, explicitly approximate
   [entrained host-volume prescription](#selected-protected-trace-prescription).
   This supplies their mechanical allocation, not chemistry or exposed phases.
4. Water/secondary mixtures and heavy-loaded H/He select the density-only
   [additive-volume contract](#selected-volatile-mixture-mechanical-contract) below
   for prescribed mixed regions. Arrangement/exposure, cold/hot component coverage
   and exchangeable partition remain separate gates. This does not select global
   miscibility or extrapolate any component EOS.

### Selected protected-trace prescription

Select `entrained_secondary_host_volume_v1` for the protected portions of
CO₂/N₂/CH₄/O₂/Ar. This is a **new modelling approximation, not a measured material
constant or solubility law**. The headless
[dry-host](planet-composition.md#headless-protected-traces-in-dry-hosts) and
[water-bearing-host](planet-composition.md#headless-protected-traces-in-water-bearing-hosts)
APIs implement this mechanical prescription only, not exposed secondary phases.
It closes one important generated-domain gap without inventing pure trace shells:

1. Host mass `M_h` is retained metal + silicate + protected water. Exposed water,
   gas/H/He and exposed secondary condensates are excluded. Prescribed heavy
   regions retain their distinct host EOS/phase models. Require `M_h>0`.
2. Let protected secondary-origin mass be `M_i`. Allocate it proportionally to
   host mass: a host parcel `dm_h` receives `dm_i=(M_i/M_h) dm_h`. Never spread
   protected material into exchangeable atmosphere, move it across the protection
   boundary, or add it again as a condensed reservoir.
3. Every resulting parcel participates with **all** allocated mass in enclosed
   mass, self-gravity and inertia. At its P/T, the selected reference specific
   volume of each entrained origin equals the host's specific volume. Consequently
   `dV=(dm_h+Σdm_i)/ρ_host(P,T)`. It is not zero-volume ballast and not removed
   from the gravity source. Solve the heavier parcel, not a massless host profile
   with density/mass repaired afterwards.
4. Domain: `f=ΣM_i/(M_h+ΣM_i) ≤0.00301`, plus the host's actual P/T validity mask.
   Natural formation has at most 0.0027 protected secondary mass per source heavy
   mass and at least `(1-0.003-0.6e-5)` host mass, before common cap scaling.
   The resulting f is <0.00301 even if all accessible supply escapes. This remains
   true under arbitrary H/He dilution because H/He are not the host denominator.
   O₂ participates under the same fraction limit for manually supplied records.
5. Secondary-origin phase/chemical binding stays **unresolved, protected,
   mechanically entrained**. Do not claim molecular survival, dissolved fluids,
   mineral binding, clathrates or a stable liquid/solid trace phase. The host's
   supported phase remains independently reportable. This route cannot create
   an exposed liquid top, new phase interface or landable floor.
6. Outside the concentration or host mask, require a separately selected model.
   Pure-secondary endpoints and exposed liquids cannot use this prescription.
   It does not license a generic default density for a new constituent.

Acceptance includes exact per-origin allocation, pure-host limit, reverse-order
regeneration and B5's concentration endpoints. Sensitivity tests vary the entrained
density ratio `λ=ρ_trace/ρ_host` through 0.5/1/2, giving
`ρ_parcel=ρ_host/[1-f+f/λ]`. At f=0.00301 the local-density variation is at most
0.301% relative to λ=1. Require global radius change ≤1% and dimensionless inertia
change ≤2% in the full generated-trace fixtures; otherwise revisit this approximation
before integration. This is an explicit mechanical sensitivity test, **not proof
that unknown true trace densities fall within that factor-two bracket**. No cp,
entropy, equilibrium partition coefficient or thermodynamic consistency is claimed
for the entrained species. Later thermal prescriptions must not average invented
host entropy into their chemical state.

### Selected pure secondary-fluid and coexistence contract

Select `pure_secondary_helmholtz_v1` using the five pinned coefficient files below.
This selects the **equations and branch/closure policy**, not a finished validity
mask, solver implementation or dense-mixture model. Published primary domains:

| Origin | Reference | Triple T, K | Primary upper T, K | Primary upper P, Pa |
| --- | --- | ---: | ---: | ---: |
| N₂ | [Span et al. 2000](https://doi.org/10.1063/1.1349047) | 63.151 | 1,000 | 2.2e9 |
| CO₂ | [Span & Wagner 1996](https://doi.org/10.1063/1.555991) | 216.592 | 1,100 | 8e8 |
| CH₄ | [Setzmann & Wagner 1991](https://doi.org/10.1063/1.555898) | 90.6941 | 625 | 1e9 |
| O₂ | [Stewart, Jacobsen & Wagner 1991](https://doi.org/10.1063/1.555897) | 54.361 | 300 | 8e7 |
| Ar | [Tegeler, Span & Wagner 1999](https://doi.org/10.1063/1.556037) | 83.806 | 700 | 1e9 |

These are ceilings intersected with the stable fluid region above freezing, not
rectangular permissions or EOS accuracy guarantees. N₂'s limited-data validation
at 1,000–1,800 K is not selected here. O₂'s ideal-gas caloric extension to 2,000 K
does not extend its dense-fluid validation. The liquid/vapour equations supply no
solid density, sublimation law, dissociation or high-pressure chemistry.

- The independent variables are `δ=n/n_r` and `τ=T_r/T`, with molar density
  `n` in mol/m³. `α=a_molar/(R_fit T)=α⁰+αʳ`. Use each file's fitted R, molar
  mass and **EOS reducing state**, not a modern replacement R or the library's
  separately reported mathematical critical density. Convert `ρ=n M` once.
  Pressure is `P=n R_fit T(1+δ αʳ_δ)`; thermal capabilities require the ideal part
  and all relevant derivatives as well. A pressure-only transcription is not
  a caloric implementation.
- Invert only on mechanically stable branches (`∂P/∂n|T>0`); cp/cv must also
  be positive when requested. When multiple stable fluid roots exist, choose
  the lowest Gibbs energy, not the first root or the higher density by default.
  This compares **fluid** roots only: it cannot exclude a more stable solid
  outside the independently established freezing mask.
- For a pure fluid below its critical region, saturation solves equal P and
  molar Gibbs energy at distinct stable liquid/vapour densities. Ancillary
  `pS/rhoL/rhoV` curves are brackets/initial estimates, not mutually exact phase
  equilibrium. Verify both residuals with the same Helmholtz equation. Do not
  assign a bulk saturation density by averaging the two densities.
- At coexistence select `pure_inventory_volume_closure_v1`: a parcel supplied
  with mass m, temperature T and volume V has `v=V/m`. Given consistent
  saturated `v_l=1/ρ_l`, `v_v=1/ρ_v`, its vapour **mass** fraction is
  `q=(v-v_l)/(v_v-v_l)`, with `0≤q≤1`, `m_v=q m`, `m_l=(1-q)m`.
  The specific volume is `(1-q)v_l+q v_v`. This is the standard lever rule,
  not an assumed 50:50 split or a volume-fraction interpretation of q.
  Outside the interval, solve the appropriate single-phase state; do not clip q
  and keep an inconsistent P/T/V combination.
- A radial model may instead prescribe T at an exposed interface and close
  the partition with retained inventory and integrated atmosphere/shell volumes.
  It must solve the available liquid and gas masses together. At pure-fluid
  saturation **P/T alone remains underdetermined**; without the additional
  mass/volume or energy constraint report missing closure, never pick a phase
  fraction by pressure convention. The planet-wide thermal anchor is still open.
- Liquid is a global pure shell only when the arrangement makes it exposed and
  its allocated mass is positive. Integrate `dV=dm/ρ(P,T)`; the constant-density
  check is `r_top³-r_bottom³=3 m_l/(4πρ_l)`. Neither the saturated vapour density
  nor the inventory including protected mass belongs in that liquid volume.
  Changing q moves a real phase boundary; changing source provenance alone does not.
- At the critical limit the density contrast vanishes and the lever rule becomes
  ill-conditioned. Resolve one continuous fluid interval, or report the critical
  resolution limit while preserving mass/volume. Do not fabricate a finite liquid
  shell by dividing by a tiny contrast. Numerical conditioning is selected below;
  the physical critical-neighbourhood mask remains open, especially for oxygen.
- These equations apply to **pure parcels**, not to a species evaluated at its
  partial pressure inside an arbitrarily dense mixture. For a liquid under a
  carrier gas, equilibrium uses fugacity/activity and the liquid's total mechanical
  pressure. The ideal dilute limit `y_i P≈p_sat,i(T)` additionally neglects gas
  nonideality, solution activity and the Poynting factor
  `exp[∫ v_molar,liquid dP/(R T)]`. A small total secondary mass fraction does not
  bound these effects. No such mixed-interface approximation is selected by this
  pure-fluid decision; dense mixtures and mixed exposed liquids remain open.

For a concrete local closure fixture use the pinned methane coexistence state
`T=95.63445009399886 K`, `P=21323.012255274934 Pa`,
`n_l=27728.839226085434 mol/m³`, `n_v=27.137532045893515 mol/m³`,
`M=0.0160428 kg/mol`. Set m=1 kg and q=0/0.25/0.5/0.75/1; construct V from
the lever rule, then recover each q and exact origin mass. Add V just outside
each endpoint and missing-V cases, which must not return a clamped or arbitrary
coexistence fraction. This is an EOS-reference algebra fixture, not a simulated
planet or independent laboratory measurement. B3/B9 still need their global
thermal and mixed-atmosphere inputs before they become complete planet fixtures.

### Selected volatile-mixture mechanical contract

Select `prescribed_volatile_additive_volume_v1` for the **mechanics of a supplied
mixed region**, not automatic allocation of all volatile supply to one mixture.
This is an explicit density approximation. It is neither a complete replacement
route nor an equilibrium mixture EOS until the arrangement, component masks and
thermal path have also been supplied:

1. Inputs are local total mechanical P in Pa, common T in K, and nonnegative
   allocated origin masses in kg. Evaluate every positive-mass component at the
   **same total P/T**, inside its independently selected density mask. Missing or
   invalid density for even a tiny positive allocation fails the region. An absent
   origin requires no evaluation. Never substitute a partial pressure in a dense
   pure-fluid EOS or evaluate at each component's separate planet mass.
2. Use `V=Σm_i/ρ_i(P,T)`, `ρ_region=Σm_i/V`, equivalently
   `v_region=Σw_i/ρ_i`. Preserve the input masses exactly as origin allocations;
   volume fractions `(m_i/ρ_i)/V` are not mass or mole fractions. Positive finite
   mass, volume and density are required. There is no default density and no
   averaging of critical constants.
3. Treat a nonideal H/He mixture as **one block**. With `Z` the local non-H/He mass
   fraction and `Y=M_He/(M_H+M_He)`, use
   `v_region=(1-Z)v_HHe(P,T,Y)+Σ_heavy w_i/ρ_i(P,T)`. The selected HG23 correction
   belongs inside `v_HHe` once, with X/Y normalised to the H/He block. Do not repeat
   it with whole-region H/He fractions, add another ideal-mixing correction to it,
   or absorb explicit water/secondary masses into effective Y. If no H/He remains,
   skip the block rather than evaluate undefined Y. This preserves stripping and
   pure endpoints without treating an ideal molecular gas as a giant interior.
4. Protected secondary origins retain their separately selected host-entrainment
   model; they are not charged again to this region. Metal/silicate do not become
   dissolved envelope material through this prescription. A component at pure
   coexistence must first obtain its mass/volume closure; evaluating an arbitrary
   saturated density to complete the sum is forbidden.
5. This capability is density-only, over the intersection of the active component
   masks and any selected H/He block mask. All mass fractions are algebraically
   allowed; that is **not a claim of measured accuracy for all compositions**.
   No cross-species excess volume, dissolution, chemical binding, entropy, energy,
   latent heat or mixture adiabat is supplied. Even positive individual cp values
   do not establish stable mixture thermodynamics. A prescribed T(P) is required
   until a separately selected thermal capability supports more.
6. Arrangement is an independent input: a mixed region records **prescribed
   mechanically mixed, mixture phase unresolved**. Pure-component phase labels can
   be retained as reference evaluations, never copied into a solved mixture phase
   or a landable floor. Separate regions record **prescribed segregation**, not
   predicted immiscibility. An exposed pure condensate must have its own positive
   allocated mass, pure-phase support and exposure condition. Neither a dense
   mixture nor its trace water/methane content creates a pure liquid surface.

Scientific basis: [Bethkenhagen et al. 2017, equation 1 and sections III/V](https://arxiv.org/html/1709.04133v1)
describe additive volume at common P/T and test selected planetary-ice mixtures.
Their roughly 4% maximum density deviations concern their specific DFT datasets,
compositions and conditions, **not an error bound for our five secondary species,
H/He enrichment, cryogenic interfaces or substituted component EOSs**. No table
from that paper has been imported or licensed for redistribution here.
[Pruteanu et al. 2017](https://doi.org/10.1126/sciadv.1700240) report a strong
increase of methane solubility in water near 1–2 GPa (373 K measurements). This
explicitly rules out using the word “hydrophobic” to justify permanent separation
throughout a planet. Their measurements do not extend Setzmann–Wagner beyond 1 GPa
or define a universal methane/water demixing curve. The [public full text](https://pmc.ncbi.nlm.nih.gov/articles/PMC5567757/)
is a reference, not an imported data dependency.

**Allocation policy selected for the cold methane fixture's water-bearing variant
B3w only:** prescribe a
water-ice substrate and a separate exchangeable pure-CH₄ reservoir, with no
dissolution, clathrate formation or reaction. At 94 K this is not two freely mixing
liquids: the water substrate must independently pass its ice density/phase mask.
Apply methane's pure-fluid phase model at its actual mechanical load and conserve
its vapour/condensate allocation together. Report prescribed segregation and
neglected hydrate chemistry, not an observed Titan reconstruction. A nitrogen
carrier atmosphere would change the partition problem and is not implied by B3's
1.5e5 Pa load. This selects arrangement, not B3's unfinished global thermal and
atmospheric closure. Do not promote it to a universal generated arrangement.
B3 itself retains its original silicate substrate; B3w is an additional request,
not a replacement of that acceptance case.

For mixed atmospheres, keep the full fugacity/activity obligation. The proposed
ideal dilute interface shortcut may be accepted only after documenting gas
nonideality, liquid activity and pressure-work errors **separately**. Select a
screening ceiling `|ln(Poynting factor)|≤0.01` for neglecting the last effect:
`|∫v_m,l dP/(R T)|≤v_m,max |P-p_sat|/(R T)`. Here `v_m,max` bounds supported
liquid molar volume across the entire integration interval at that T. A single
saturated density is not automatically such a bound. This 0.01 is an adjustable
approximation-screening tolerance (factor error ≤exp(0.01)-1 ≈1.005%), not a
measured constant or a complete 1% equilibrium error budget. Passing it does not
bound gas fugacity or activity. Failing it requires pressure correction or another
selected partition model, never silently dropping accessible supply. No gas-pair
error mask or complete ideal mixed-interface route is selected in this pass.

Finite **algebraic** requests B11/B12 below supplement, not replace, the planet
fixtures. The offline secondary audit tests volume addition, active trace
preservation, pure limits, H/He-block normalisation and liquid pressure-work units.
They are not demonstrations that generated dense mixtures fit the component masks.

### Selected phase-resolution policy

Select `pure_phase_conditioning_v1`, a numerical policy, not a widened EOS mask:

- The reducing/nominal critical temperature is not the fitted equation's terminal
  coexistence temperature. Retain both; bracket against the mathematical critical
  state with independently checked stable roots. Oxygen's two-root search must
  remain eligible between 154.581 and 154.59938983528554 K. Source superancillary
  critical metadata are a reference for the search, not independent physical data.
- For consistent saturated specific volumes define `c=(v_v-v_l)/max(v_l,v_v)`.
  Require `c>1e-6` to evaluate a resolved lever-rule fraction. Otherwise preserve
  the inventory and report **unresolved coexistence contrast**, not automatically
  “single fluid”. Only a supported branch/stability result may establish the latter.
  The 1e-6 contrast floor is a numerical control, not the size of a physical
  critical region. Both reference phases must still pass their physical masks.
- The contrast floor alone is insufficient. Bound the absolute errors `e_v`,
  `e_l`, `e_g` in supplied/solved bulk, liquid and vapour specific volumes. Require
  `D=v_v-v_l>e_l+e_g` and the conservative ratio error bound
  `[e_v+e_l+|q|(e_l+e_g)]/[D-e_l-e_g]≤1e-4` for reporting q. This follows by
  bounding numerator/denominator perturbations in `q=(v-v_l)/D`. Include inversion,
  coexistence and volume-matching error estimates, not machine epsilon alone.
  The 1e-4 is an absolute vapour-mass-fraction resolution target, not EOS accuracy.
  Refine within the shared work ceiling; if it cannot pass, report the unresolved
  partition without manufacturing an interface or discarding either origin mass.
- Do not clamp out-of-range q, move temperatures across a boundary, or snap a
  source endpoint into a different physical state. Coordinate roundoff may be
  handled only with an explicitly recorded input uncertainty and one-sided checks.
  O₂/Ar freezing-edge discrepancies remain unresolved physical-mask questions;
  this conditioning policy does not reconcile or erase them.

## Pinned source checks

Downloads are ignored under `data/composition-sources/`. No runtime dependency or
upstream data/code has been imported into tracked files. Retrieve Zenodo files at
`https://zenodo.org/api/records/{record}/files/{filename}/content` and verify with
`sha256sum`. Inspect archives with `tar -tzf`, read named members with `tar -xOzf`;
do not extract arbitrary paths. Hashes identify bytes, not scientific fitness.

| Record / file | Bytes | SHA-256 |
| --- | ---: | --- |
| [19135021](https://doi.org/10.5281/zenodo.19135021), `EOS_Chabrier2021_HHe.tar.gz` | 22,747,679 | `579658d918611f45e2ebc25c92bcb6c9d1d954b4f769897bff7c560c11f30507` |
| [12514103](https://doi.org/10.5281/zenodo.12514103), `HG23_Vmix_Smix.csv` | 1,830,098 | `86cf66c9551cce76eaff4e04f6746f30150cf3bcb10217d302d283c1f06d9bfe` |
| Same record, `HG23_Vmix_Smix_Umix.csv` | 2,120,820 | `bfd8b08a7869bd7f6b90d8d7c20c3ef7504c88a7ff67715ba78952114f320eb2` |

All have CC-BY-4.0 in Zenodo metadata. Latest HG23 is 12514103, not 7346181;
it adds energy corrections and preserves the volume/entropy file bytes. Attribute
original authors/converters, retain notices, record modifications and check
upstream provenance before redistribution. A third-party record's licence alone
does not settle every original-author permission question.

H/He archive: original June-2021 v1 TP/Trho tables and five PALEOS conversions.
Its README advertises effective-H/conversion files **absent from this archive**.
Reproduce the actual manifest. Documented grid: 121×441, 100–10⁸ K, 1–10²² Pa;
original README excludes solid/ion-quantum-invalid states. Converted phase labels
are heuristic: helium labelled `molecular` is not molecular, nor is `dissociating`
a helium phase diagnosis. The converter warns of clamped `nabla_ad`, unreliable
cp/cv, negative cp beyond flagged cells and NaN u/s inside its nominal mask.
These are source warnings, not independent numerical validation here. Audit
original density/entropy, signs and derivatives before thermal use. Conversions:
GPa→Pa ×10⁹, g/cm³→kg/m³ ×10³, MJ/kg→J/kg and MJ/(kg K)→J/(kg K) ×10⁶,
**after** base-10 log decoding. The reproducible audit below now checks the
original grids and the HG23 volume/entropy unit conversion.

### Independent H/He reference audit

```sh
python scripts/audit-hhe-reference.py data/composition-sources > data/composition-sources/hhe-audit.json
PYTHONDONTWRITEBYTECODE=1 python scripts/test-hhe-reference.py
```

The standard-library script has no network access, does not extract the archive,
checks all three outer SHA-256 pins, and reads only named original TP members.
It rejects duplicate/missing/nonfinite/malformed coordinates and verifies that
both HG23 files retain the same volume/entropy columns. All three original grids
and both correction grids contain 53,361 points. Original member SHA-256:

| Original member | SHA-256 |
| --- | --- |
| `TABLE_H_TP_v1` | `0215333ff5d727e9059dc474ef2b615e3dee62af5a16a2dab037ee181a2ff892` |
| `TABLE_HE_TP_v1` | `fae4720798f6189f5d10020a996c264b72d72406be40b15175a0b9799efc932b` |
| `TABLEEOS_2021_TP_Y0275_v1` | `c4995d114affedddf421b57b847ad4872699e9526f32b4b335013ffcbfb0b938` |

HG23 equations 1/2/4 and Appendix B specify:

- `P_Pa=10^(LOGP-1)` from dyn/cm²; `T_K=10^LOGT`.
- `Vmix_SI=1e-3 Vmix_cgs` (cm³/g to m³/kg).
- `Smix_SI=1e-4 Smix_cgs` (erg/(g K) to J/(kg K)).
- `1/ρ=X/ρ_H+Y/ρ_He+XY Vmix`, `s=X s_H+Y s_He+XY Smix`.
  **Smix already includes the approximated ideal mixing contribution**. Do not
  add an ideal mixing entropy a second time. X/Y are H/He-submixture mass fractions,
  not whole-planet fractions including heavy material. Pure endpoints recover
  CMS19. This is a published approximate mixture construction, not a single
  fitted potential guaranteeing every derivative identity after interpolation.

References: [HG23 full equations and printed table](https://arxiv.org/html/2302.07902v1),
[CMS19 construction, masks and thermal identities](https://arxiv.org/html/1902.01852v1).
The added Umix column is preserved/hashed but **not selected for an energy or
cooling calculation**: the primary HG23 paper does not document that later column.

An independent comparison finds the printed HG23 Appendix-B values are **not
identical to the pinned CSV**. At logP(cgs)=10.5 and logT=2.5/2.85, downloaded
Vmix is 7.7848358% higher and Smix 2.7426381%/3.0965566% higher. The Vmix ratio
equals `(0.725×0.275)/(0.755×0.245)`: evidence of the paper's reference versus
interaction-abundance normalisations, not licence to rescale downloaded values
blindly. The published table is not a golden byte-for-byte reference. Retain this
discrepancy and test equation 5 against the original CD21 data.

That check uses pure terms at Y*=0.275 but correction amplitude
`X_tilde Y_tilde=0.755×0.245`. At source knots (10¹¹ Pa,5011.872 K) and
(10¹² Pa,19952.623 K), reconstructed CD21 specific volume agrees to floating-point
roundoff; at (10⁶ Pa,1000 K) residual is 1.11242e-5, consistent with cleaned/small
corrections not being an exact inverse. The generated Y=0.25 mixture densities
there are respectively 761.474, 1842.102 and 0.276323 kg/m³. These are **numerical
reference values, not certifications that each pure constituent is fluid there**.

The source paper provides useful independent phase screens: H₂ equation 1,
`T_m=4.853 (1+P_GPa/0.023)^0.748 exp(-0.0098 P_GPa)`; He equation 7,
`T_m=61 (P_kbar)^0.639 K`. The audit uses these only in restricted high-pressure
comparison intervals (0.1–300 GPa for H₂, 0.1–100 GPa for He), not as universal
low-pressure/ionised phase selectors. At the 100 GPa/5011.872 K source knot,
the pure-He melting estimate is 5038.831 K: **the helium state is below this
pure-fluid mask despite its finite tabulated density**. This is an intentional
negative phase test, not a mandatory successful fluid state. It also cannot be
used to infer a real mixture's melting boundary without a mixture prescription.
Add a 100 GPa/10,000 K positive companion fixture. Higher pressures still need
the source's plasma/quantum masks and an explicitly bounded low-temperature route;
do not extrapolate these molecular melting curves indefinitely.

The original tables independently confirm the thermal cautions. Within the
inspection window 100–10⁵ K / 100–10¹⁴ Pa (14,701 points per table):

| Numerical diagnostic | H | He | CD21 Y=0.275 |
| --- | ---: | ---: | ---: |
| Nonpositive supplied compressibility derivative | 275 | 388 | 395 |
| Nonpositive/unusable `cp=s(dlns/dlnT)_P` | 535 | 175 | 678 |
| Nonpositive/unusable cp from density/gradient identity | 1,317 | 569 | 965 |
| Supplied `nabla_ad` exactly 0.1 | 1,641 | 1,126 | 1,665 |

Central entropy differences independently recover an ideal-gas test surface and
show non-negligible table-gradient disagreements in this window. **This window
is not a physical mask**: some warnings are in solid/invalid regions. Do not infer
a defect count for physically valid states from this table. Globally, original
pure-H log(u)/log(s) have 56/40 undecodable overflow/underflow points; pure-He has
19/1. Such points cannot become usable zero values. A thermal path needs the
physical mask plus stable interpolation/derivative checks; merely switching from
the converted table to originals does not close thermal coverage.

CoolProp file prefix: `https://raw.githubusercontent.com/CoolProp/CoolProp/v8.0.0/`.

| Path | SHA-256 |
| --- | --- |
| `LICENSE` | `9bf835333ef602af4cb19338b9f9d43671e174fa029b00280e9bdba6ea4719b2` |
| `dev/fluids/CarbonDioxide.json` | `08e27e1a5a6029e508976496b9d1fd4f7f0caa2c46fd35c84b7bf520260a5217` |
| `dev/fluids/Methane.json` | `534b6a8906be63dd491972ddb29f2856c565c2c209320044858e67b90cb7a171` |
| `dev/fluids/Nitrogen.json` | `791432d1685eafcc75b43e776e8fe4e30f7cf73a2a73b67de0c1a94c57a2b9aa` |
| `dev/fluids/Oxygen.json` | `2296d3164338a9fadeb2e7a04c98fb5c403a07851c3b59bb7634887043c44fc3` |
| `dev/fluids/Argon.json` | `3dd2932a954084990add53fe583d48ff2d5ab99f9efa57b7f997377d7ea5322a` |

Retrieved licence is MIT: retain copyright/permission notice. These coefficients,
citations and ancillary curves do not confer unrestricted physical validity.
Keep fitted gas constants/molar masses for equation checks, not substituted
modern constants. Melting curves supply no solid EOS. This is not a licence audit
of every future CoolProp dependency. Revised AQUA's separate hash/schema/phase
audit remains in the [water guide](water-physics.md#revised-aqua-2026-distributable-candidate-not-interface-ready).

### Independent secondary-fluid reference audit

```sh
python scripts/audit-secondary-reference.py data/composition-sources > data/composition-sources/secondary-audit.json
PYTHONDONTWRITEBYTECODE=1 python scripts/test-secondary-reference.py
```

The standard-library audit verifies the five coefficient hashes and licence hash,
transcribes residual power/Gaussian/CO₂ nonanalytic terms, and evaluates supplied
T/molar-density states only. It performs no inversion, structure solve, runtime
data conversion or library installation. Ten synthetic regressions check ideal
units, analytic derivatives against differences, malformed coefficients, three
distinct melting-curve conventions, ancillary intervals and primary ceilings.
This audits pressure and same-T Gibbs differences, **not u/s/cp/cv or the complete
phase diagram**. At the same T the ideal T-only Helmholtz terms cancel, leaving
`Δg/(R T)=Δ[ln δ+αʳ+1+δ αʳ_δ]`.

The suite contains 19 offline regressions, including synthetic
additive-volume/H-He grouping checks, unclipped lever-rule endpoints/interiors,
critical contrast/error-bound rejection and molar liquid pressure-work units. The full audit
also recovers five methane vapour mass fractions from the pinned local coexistence
row and constructed volumes. These are algebra/transcription checks only; the
phase-resolution error estimates still require solver-level acceptance. Synthetic
endpoint perturbations check the conservative ratio bound, not physical EOS errors.

- All 15 source-supplied saturation rows satisfy pressure matching to <3e-11
  relative and `|Δg|/(R T)<3e-14` under this independent transcription. CO₂'s
  source lists its triple row twice, so these are **14 distinct states**, not 15
  independent observations. These internal source checks are not laboratory
  validation or proof of global stability against solids.
- The stored oxygen and argon `sat_min_vapor` anchor pressures differ from the
  transcribed EOS by 7.45136e-6 and 4.25708e-6 relative. Report those explicitly
  as failing the proposed 1e-6 transcription target; do not relax that target,
  rescale coefficients or use the rounded anchor as exact coexistence. Their
  separate supplied saturation check rows satisfy the much tighter equal-P/g
  checks above. The discrepancy's provenance has not been resolved.
- Independently fitted ancillary curves differ from supplied saturation values
  by up to about 2.1e-5 relative in pressure and 1.1e-3 in density at the tested
  in-domain states. They cannot meet the solver's tighter equality residuals as
  exact substitutes. A source temperature a few ulps outside its ancillary
  interval is reported as outside, not silently clipped; future endpoint matching
  must distinguish coordinate roundoff from actual physical-domain extrapolation.
- O₂ has nominal/reducing `T_c=154.581 K`, but its fitted equation's mathematical
  critical temperature is `154.59938983528554 K` (from source superancillary
  metadata). The difference is ~1.19e-4 relative, much greater than B10's ±1e-6
  offsets. Therefore nominal critical constants alone must not switch off the
   two-root search or declare a continuous fluid. The selected conditioning policy
   above prevents that shortcut; its physical neighbourhood mask remains open.
- Melting correlations have their own intervals and sources: N₂ 63.151–287 K;
  CO₂ 216.592–330 K; CH₄ 90.6941–600 K (Abramson 2011, distinct from the EOS);
  O₂ 54.361–63.1 K; Ar 83.806–260 K. Do not treat their intervals as liquid EOS
  temperature ceilings, extrapolate the curves or derive a solid density from them.
- The Ar melting formula uses `T₀=83.8058 K` while the JSON lower endpoint is
  83.806 K: its lower-end pressure is 69,687.997 Pa, not the formula's reference
  68,891 Pa. The O₂ melting fit returns 17,074.273 Pa at 54.361 K, whereas the
  stored fluid triple/saturation pressure is about 146.278 Pa. These mismatches
  prevent blindly selecting either raw curve as an exact triple/freezing edge.
  Keep those boundary uncertainties visible until reconciled against the primary
  phase sources. None of the five JSON files supplies a `sublimation_line`.

Result: select the bounded pure-fluid equations and closure above, but **do not
mark secondary coverage complete**. Cryogenic solids, sublimation, freezing-edge
uncertainty, dense/exposed mixtures and deep/hot chemistry remain separate gates.

### Parked methane physics

The offline methane inversion gate and runtime methane fluid, cold-state,
condensed-shell and dilute-exchange work are parked on the
`park/methane-physics` branch. Methane is treated as an ordinary gas on the
main composition line. Methane source rows above remain reference material only.

## Finite acceptance requests

These are input requests, **not claims of current support**. Masses are Earth
masses; P/T pairs are Pa/K. Retain existing independent numerical fixtures.

| ID | Request | Required distinction |
| --- | --- | --- |
| B1 | Existing dry and (metal,silicate,water)=(0.2,0.5,0.3), 250/300 K | Closure/refinement; cold density alone leaves refractory solidity unknown. |
| B2 | B1 heavy inventory, H/He Y=0.25, separate imposed base loads 10⁸/10⁹ Pa | Supported interfaces; no pressure-triggered ground loss. Explicit unknowns. |
| B3 | 0.999 silicate + 0.001 exchangeable CH₄; 1.5e5 Pa, 85/94/120 K | Frozen/liquid/vapour neighbours; liquid top only if exposed. |
| B3w | 0.699 silicate + 0.300 protected water + 0.001 exchangeable CH₄; total 1, outer load 1.5e5 Pa, prescribed isotherms 85/94/120 K | Prescribed ice substrate and separate methane reservoir; no implicit dissolution/clathrates or liquid water. Atmosphere/inventory closure still required before a complete planet fixture. |
| B4 | Pure CO₂ local (1e5,190), (1e7,280), (1e5,280), (1e7,320) | Solid/liquid/gas/supercritical with branch checks; no liquid below triple pressure. |
| B5 | Each secondary pure and 1e-12/1e-7/0.003 of heavy mass; natural four-species ratio with 90% protection | No loss, automatic exposure or pure trace shells. Include manual O₂; protected fractions use the selected entrainment policy, pure/exposed endpoints cannot. |
| B6 | Pure H, pure He, Y=0.25/0.275; local (10⁶,1000), (10¹¹,5000/10000), (10¹²,20000) | Independently checked density/thermal paths within masks; conserved origins. Pure-He 100 GPa/5000 K must not pass as supported fluid; retain the positive warmer neighbour. |
| B7 | Heavy mass 1; gas 0/1e-8/0.01/1/100/1000, Y=0.25 | Stripping continuity/self-gravity/inertia; no one-bar surface or ideal-gas giant. Thermal anchors still to fix. |
| B8 | Total 3900, heavy 36; sampled extrema above | Full intended mass tail without silent mask extrapolation. |
| B9 | Exposed liquid 1e-12/1e-8/1e-3 of total over supported substrate, phase neighbours | Mass-derived thickness, submerged floor, liquid-top reference. Label unresolvable thickness, never inflate it. |
| B10 | Selected triple/saturation/critical boundaries and seams at relative P/T offsets ±1e-6/±1e-4 | One-sided states, disappearing contrast, no physical interface at provenance-only seam; refinement/failure. |
| B11 | Synthetic same-P/T parcels: masses (0.8,0.2) kg, densities (1000,400) kg/m³; add 1e-12 kg at 10 kg/m³; reverse order and scale all masses by 7 | V=0.0013 m³ before the trace, ρ=769.230769… kg/m³, not 880; trace volume stays positive, pure/scaling limits. Not a physical water/CH₄ state. |
| B12 | Synthetic H/He block: X=0.75, Y=0.25, Z=0.2; v_H=0.01, v_He=0.002, Vmix=-0.001 m³/kg; heavy ρ=1000 kg/m³ | v_HHe=0.0078125, v_region=0.00645 m³/kg; no whole-region XY correction or second heavy allocation. Not a physical HG23 reference point. |

The local methane mass/volume fixture in the
[pure-fluid contract](#selected-pure-secondary-fluid-and-coexistence-contract)
fixes one finite B10 coexistence input. B3's frozen/liquid/vapour labels are pure
secondary expectations at the stated mechanical load, not a prediction for a
carrier-gas mixture at that total pressure. A mixed atmosphere needs its own
composition/partition closure. Neither local test replaces the B3/B9 planet solve.

Solid boundary, liquid top, continuous fluid, pressure reference and model limit
remain independent facts. Unresolved solidity is not absence of solids. Liquid
presentation preserves phase/material and camera/landing policy. Protected liquid
at depth is not exposed. No default pressure, class or gas fraction decides ground.

## Proposed numerical and delivery budgets

These are extension acceptance targets, not changes to current controls or
performance claims. Preserve stricter local tests. Count rejected/nested work.

- Per-origin allocation residual ≤1e-12 relative plus documented subtraction
  allowance at that origin's source scale; zero supply stays zero.
- Tightened controls change radius/dimensionless inertia ≤1e-4. Independent
  shell-mass/hydrostatic quadrature residual ≤1e-3. Relative interface-radius
  location ≤1e-6 or explicit resolution limit. Keep one-sided checks.
- Equation transcription checks ≤1e-6 relative at nonsingular reference points.
  Table interpolation needs a separately justified source-grid/refinement tolerance.
- Complete structure: ≤2,000,000 local EOS evaluations, 65,536 retained samples,
  128 interface/seam events, 128 matching/partition trials. Retries share counters.
- Retention: existing ≤40 survival trials and shared request ceiling 80,000,000
  EOS evaluations. This safety ceiling is not interactive readiness; checkpoint
  10 must measure a cheaper strategy or validated accelerator before integration.
- Data: ≤64 MiB shipped compressed, ≤256 MiB resident read-only tables shared per
  process, ≤16 MiB scratch per active solve. No original/converted duplicates or
  Apple metadata. Derived data need deterministic conversion, masks, notices/hashes.
- No wall-clock correctness thresholds. CPU/search/cache costs belong to checkpoint
  10; interactive benchmarks remain user-run. Distinguish invalid input, missing
  capability, physical-domain exit, weak-binding/no-static-solution, nonconvergence
  and work exhaustion. Never silently return a legacy whole-body fit.
