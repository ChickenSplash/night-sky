# Pure-water material physics

`system::materials::water` is a headless, opt-in local pressure/temperature model.
It implements part of layered-structure checkpoint 3. A separate
[restricted radial solver](planet-composition.md#headless-restricted-water-shell-solver)
now uses its low-pressure model for water shells and supported ice-Ih/liquid
interfaces, the separate compressed-liquid model to 1 GPa within its mask and
the [effective deep-ice approximation](#effective-high-pressure-ice-for-the-game).
It changes no generated body, reservoir, atmosphere,
transport, inspector or rendering. The [dry radial solver](planet-composition.md#headless-dry-body-solver)
still refuses every positive water inventory.

## Selected baseline and physical limits

Use direct evaluation of published IAPWS equations rather than a new density
fit or a coarse interpolated table. `iapws_low_pressure_water_v1` combines:

- **Liquid density:** IAPWS R7-97(2012), IF97 region 1, equation 7 and tables 2–4.
  `v = (RT/p*) γ_π`, `ρ=1/v`, `π=p/(16.53 MPa)`, `τ=1386 K/T`.
  The formulation's fitted gas constant is 461.526 J/(kg K). Do not substitute a
  different gas constant independently of its fitted coefficients.
- **Ice-Ih density:** IAPWS R10-06(2009), equation 1 and tables 2–4.
  `ρ=1/(∂g/∂p)_T`. Only the pressure-dependent Gibbs terms contribute, so neither
  a specific entropy zero nor a thermal evolution model is implied. The complex
  logarithm in the published equation uses its principal branch; the implementation
  evaluates the required real/imaginary components directly.
- **Ice-Ih melting pressure:** IAPWS R14-08(2011), equation 1. Its temperature
  range is 251.165–273.16 K. Increasing pressure lowers the freezing temperature
  along this branch. It is not a universal freezing-temperature rule.
- **Liquid/vapour saturation:** IF97 equation 30 and table 34, used to identify
  unsupported vapour and coexistence rather than return metastable liquid density.

Public calls use SI: absolute Pa, kelvin and kg/m³. Application masks are
**250–623.15 K and 1,000–100,000,000 Pa (0.01–1,000 bar)**. These are deliberately
narrower than some source formulations. No extrapolation, clamping or fallback
proxy is used.

| Requested equilibrium state inside those masks | Phase identification | Density |
| --- | --- | --- |
| Stable ice Ih | Ice Ih | Supported |
| Stable liquid at ≥273.15 K | Liquid | Supported |
| Stable liquid below 273.15 K | Liquid | Explicit unsupported error |
| Vapour | Vapour | Explicit unsupported error |
| Ice-Ih/liquid coexistence | Named coexistence | Unconstrained density refused; named one-sided limits supported where their EOS exists |
| Liquid/vapour coexistence | Named coexistence | Explicit unsupported error |

At 250–251.165 K and ≤100 MPa the supported phase is ice Ih. The pressure floor
is above the ordinary triple-point pressure, so sublimation and three-phase
coexistence cannot occur in this application domain. Other ice phases,
supercritical states and dissociation are outside the masks, not silently
classified as Ih or liquid. Pure H₂O remains the existing molecular material
budget, with no new hydrogen or oxygen allocation, salinity or dissolved species.

`phase(p,T)` and `state(p,T)` deliberately distinguish phase knowledge from
density support. A state error includes its reason and input P–T. The standalone
`melting_pressure_ih_pa(T)` remains a compatibility entry point to the Ih branch
of the separate melting-curve model below. Neither API enables high-pressure
density or provides a complete high-pressure phase diagram.

Coexistence recognition uses 16 machine epsilons of relative pressure solely to
recognise evaluations of the same curve. It is **not a physical uncertainty
band**. The melting correlation's published pressure uncertainty is 2%; arbitrarily
close classifications on either side are nominal model results, not accurate
measurements. P–T alone cannot select phase fractions or a coexistence density.
The API does not manufacture a 50/50 mixture or smooth a density discontinuity.

`condensed_state(p,T,phase)` explicitly selects liquid or ice Ih and allows that
phase's one-sided density on the Ih melting curve. Away from coexistence, the
requested phase must be the stable one; mismatches fail rather than enabling
metastable extrapolation. The same pressure/temperature masks and subfreezing
liquid refusal apply. Both endpoint densities therefore exist only at ≥273.15 K.
This API supplies neither a bulk mixture density nor a phase fraction, and
`state(p,T)` keeps its original coexistence refusal.

## High-pressure melting curves, not high-pressure density

`water::melting`, model `iapws_r14_08_2011_melting_v1`, directly evaluates
IAPWS R14-08(2011), equations 1–5. The caller explicitly selects a **melting
branch**, not a phase inferred from pressure. Input is kelvin; output is absolute
Pa. Its domain is independent of the low-pressure density mask:

| Solid/liquid branch | Inclusive temperature range (K) | Published pressure uncertainty |
| --- | ---: | ---: |
| Ih | 251.165–273.16 | 2% |
| III | 251.165–256.164 | 3% |
| V | 256.164–273.31 | 3% |
| VI | 273.31–355 | 3% |
| VII | 355–715 | 7% |

Nonfinite or out-of-branch temperatures return errors with the supplied
temperature and no fabricated pressure. The curves are evaluated without
extrapolation, density fitting, blending or new dependencies. Rounded published
coefficients leave small differences between adjacent endpoint pressures. We
preserve those differences rather than shifting a branch to force exact joins.
They are much smaller than the scientific pressure uncertainties, but cannot
justify a machine-precision triple-point or phase-interface claim.

These are **solid/liquid coexistence curves only**. For example, 260 K has both
an Ih/liquid and a V/liquid boundary at different pressures. A temperature does
not select one universal melting pressure. At 250 K none of the branches applies;
the cold acceptance fixture needs solid/solid transitions, including ice II and
deeper phases. The 300 K VI/liquid curve supplies a future comparison target,
not a supported density at that pressure. The existing `phase` and `state`
functions still refuse all pressures above 100 MPa.

## Direct compressed-liquid extension

`water::dense_liquid`, model `iapws95_compressed_liquid_v1`, implements direct
IAPWS R6-95(2018) compressed-liquid density on **100–1,000 MPa and
273.16–623.15 K**, further restricted to the liquid side of the high-pressure
melting curves. This is a separate opt-in API, not a change to `water::state`,
`water::phase` or the original IF97 model identity. It does not enable ice
densities or the subfreezing liquid needed by colder phase paths.

Pressure follows the published Helmholtz formulation:
`P = ρRT [1 + δ(∂φʳ/∂δ)τ]`, `δ=ρ/(322 kg m⁻³)`, `τ=647.096 K/T`, with the
formulation's fitted `R=461.51805 J/(kg K)`. All 56 residual terms are retained,
including Gaussian and nonanalytic terms. Only the residual energy and first
density derivative are implemented; this is **not** a public entropy, Gibbs
potential, heat-capacity or general-purpose fluid EOS API.

The liquid mask uses branch V up to and including 273.31 K, then VI through
355 K. Above 355 K the high-pressure melting line lies beyond the 1 GPa ceiling.
At these pressures and below 623.15 K there is no vapour equilibrium. Requests
on the nominal melting curve return coexistence errors; requests above it return
solid-domain errors without inventing a deeper ice label. Published curve
uncertainties and endpoint rounding still apply. This mask combines published
formulations, not a full Gibbs-minimisation phase diagram.

Bounded bisection in density takes at most 96 iterations plus two bracket
evaluations. One additional IF97 density evaluation at 100 MPa supplies only a numerical seed, never an
answer, fallback or blending weight. Initial bracket endpoints are 0.98 and
1.6 times that seed; their evaluated pressures are algebraic inversion probes,
not certified physical states. A returned density must close the actual requested
pressure to `max(0.01 Pa, 10⁻¹¹ P)`. Failed brackets or exhausted iterations are
explicit typed errors retaining input P–T. No pressure shift or profile repair is
used. The budgeted entry point reports work on success and failure, counts the
seed and every pressure probe, and cannot raise the fixed 99-evaluation ceiling.
The radial caller debits all of this work in addition to its density dispatch,
including initial outer-state validation and unsuccessful shooting trials.

At the shared 100 MPa endpoint, IAPWS95 and IF97 are compared but not forced to
agree exactly. The radial solver now locates this **model boundary** with separate
one-sided densities and disjoint liquid-region allocations, using the existing
pressure-coordinate endpoint integrator. No blending, density matching, phase
change or thermodynamic-potential matching is implied. The original local APIs
remain separate. All generated bodies and dry-solver coverage remain unchanged.

## Prescribed ice-VII density branch

`water::ice_vii`, model `lai_2023_ice_vii_300k_bm3_v1`, implements the
**fixed-volume third-order Birch–Murnaghan** fit of
[Lai et al. (2023), American Mineralogist 108, 1530–1537](https://doi.org/10.2138/am-2022-8554).
The public mask is **exactly 300 K and 3.5–20 GPa**. This is an explicitly
requested compression branch, not an equilibrium phase selector. Results have a
separate `BranchDensity` type, not `water::State`; no high-pressure variant is
added to the original `Phase` enum. In particular a 250 K request is rejected,
not evaluated as 300 K or relabelled ice VIII.

The equation is
`P = (3K₀/2)(x⁷−x⁵)[1+(3/4)(K′₀−4)(x²−1)]`, `x=(V₀/V)^(1/3)`.
Use the **same fitted parameter row** throughout: `K₀=21.0 GPa`, `K′₀=4.45`,
and fixed cubic unit-cell volume `V₀=40.86 Å³` (figure 5 caption).
The table-1 molar volume, 12.3 cm³/mol, is more coarsely rounded and is not
substituted into this parameter set. With two H₂O molecules per cubic cell,
`ρ₀=2M/(NₐV₀)` and `ρ=ρ₀x³`; the mass conversion uses
`M=0.01801528 kg/mol` and exact SI `Nₐ=6.02214076×10²³ mol⁻¹`.
Neither the separately fitted-volume BM3 parameters nor the Vinet parameters
are mixed into this model.

For `x≥1`, the pressure factors are nonnegative and increasing, establishing a
unique compressive root. Bisection brackets it with algebraic probes at
`x=1` and `x=1.2`. These probes do not expose out-of-mask physical states.
Accepted forward pressure residual is at most `10⁻¹¹ P`. There are at most
98 pressure evaluations, including endpoint probes. The budgeted entry point
can reduce, not raise, this ceiling; both success and failure report consumed
evaluations. An exhausted budget never returns a partial density. This makes
nested inversion work accountable without yet integrating a planet.

The study measured 3.5–78.2 GPa and 300–1,000 K, but those extrema are not a
rectangular stable-phase domain. Its thermal fit is not implemented, and its
discussion of disputed structural changes is not a phase diagram. The 20 GPa
application ceiling avoids using its broad smooth compression fit to declare
VII/X interfaces. Even inside this narrower mask, this API supplies the requested
branch only, not a proof excluding every competing structure. There is no Gibbs
potential, VI/VII equality, ordered-VIII EOS or deep-water fallback.

The paper quotes fitted uncertainties of 0.4 GPa in `K₀` and 0.06 in `K′₀`
for this row. A fixed reference volume is a fit constraint, not a zero-uncertainty
measurement. The numerical root tolerance is not the physical density accuracy.
The two H₂O cubic lattice observations printed on page 1532 give a useful
transcription check: at 3.5 GPa the fit's cell volume is 0.1624% lower than
`(3.3046 Å)³`; at 5.5 GPa it is 0.3206% lower than `(3.2510 Å)³`.
These observations belong to the fitted study, not independent validation data.
They establish neither agreement within experimental uncertainties nor uniform
0.4% accuracy across the mask.

Only mathematical relations, their numerical parameters and two cited scientific
observations are transcribed. No article, figure, third-party implementation,
interpolation table or SeaFreeze asset is distributed as part of this model.
The consulted [author-hosted PDF](https://zhangdongzhou.github.io/pub/Lai_AM_2023.pdf)
has SHA-256 `57ac31861983646faa73a754998a5d93a88abc50681dba78b8352c869cf99e5a`.
Availability of that PDF is not asserted to be a permissive software/data licence.

## Effective high-pressure ice for the game

`water::deep_ice`, model `seager_2007_effective_high_pressure_ice_v1`, implements
[Seager et al. (2007)](https://arxiv.org/html/0707.2895), equation 11/table 3,
using the printed SI H₂O row: `ρ₀=1460 kg/m³`, `c=0.00311`, `n=0.513`,
`ρ(P)=ρ₀+cPⁿ`. Only the equation and constants are transcribed, not third-party
code or tables. The paper fits a merged compression EOS below 10¹⁶ Pa; our
application stops at **10¹³ Pa**. It is not the paper's detailed full EOS.

The combined temperature mask is **250–300 K**. At ≥273.15 K, the existing
IAPWS V melting branch through 273.31 K and VI branch above it supply the lower
pressure bound. This follows the liquid model's choice at the rounded V/VI seam,
rather than leaving a temperature gap there. Below 273.15 K, deeper ice starts
at the rounded 2.1 GPa boundary, above the intermediate-ice prescription below.
The fit
itself is temperature-independent: the supplied temperature selects the
approximate boundary, not a claimed thermal correction. Tests check independent
55-digit Decimal evaluations, positive compression, inverse algebra and masks.

This is **effective high-pressure ice**, not an ice-VI density EOS or resolved
VI/VII/VIII sequence. In particular its roughly 1,589 kg/m³ density at 1 GPa
overestimates intermediate ice-VI density. The 1.5 GPa independent lattice
observation documented below is not a calibration of this approximation.
The proxy prioritises plausible bulk compression and a bounded, usable planetary
solver over detailed intermediate shells. No uniform density error bar is known.
Joining this compression proxy to an empirical melting line is an approximate
composite model, not Gibbs-energy matching. Interface density values are not
adjusted or blended to hide the mismatch.

The 300 K/30%-water acceptance fixture now solves, with independent conservation,
hydrostatic and refinement tests in the [solver guide](planet-composition.md#effective-deep-ice-and-checkpoint-3-acceptance).
The 250 K deep path now uses the labelled cold approximation below, rather than
refusing the whole planet. Atmosphere matching and temperature evolution remain
later checkpoints. The original local IAPWS APIs/model identities are preserved;
`dense_liquid::state` still refuses coexistence, while the explicit
`liquid_limit_with_budget` returns only the named liquid-side limit.

### Cold intermediate ice approximation

`water::cold_ice` covers **250–273.15 K** (exclusive upper endpoint). Below the
minimum liquid melting temperature, 251.165 K, it uses an all-solid path. Above
that it includes the pressure-melted liquid interval described below. Rounded constant
engineering transitions are **220 MPa** from Ih to effective intermediate ice,
and **2.1 GPa** from intermediate to the Seager effective deep-ice proxy. The
220 MPa choice is informed by the already documented 250 K reference phase
crossing near 219.94 MPa; neither threshold is a measured equilibrium curve
across the mask, and neither resolves individual intermediate polymorphs.
The rounded Ih/III endpoints differ by about 566 Pa. A liquid wedge of at most
10 kPa near that triple point is treated as unresolved within the all-solid
approximation, with an explicit metadata note. This is a declared physical-model
resolution, far below the melting-curve pressure uncertainty, not a pressure
repair or a failure-triggered fallback. All retained water remains allocated.

The original IAPWS API and model remain unchanged below 100 MPa. Between
100–220 MPa, `iapws_ih_extended_cold_pressure_proxy_v1` explicitly extends the
same Ih density equation to the rounded approximate transition. At 100 MPa
the two densities agree because they use the same equation, not a fitted offset.
The small extension near the Ih stability limit is labelled approximate.

`reference_anchored_cold_intermediate_ice_v1` uses Murnaghan compression:

```text
rho(P) = rho_ref [1 + K' (P - P_ref)/K_ref]^(1/K')
rho_ref = 1386.12517 kg/m³, P_ref = 1.5 GPa
K_ref = 14 GPa (assumed), K' = 4 (assumed)
```

The density anchor is the previously audited Chodkiewicz et al. (2022) H₂O
lattice observation at **293(2) K**, not a 250 K measurement. Elastic constants
are engineering assumptions of plausible ice stiffness, not parameters measured
or fitted by that study. Reusing the reference without a thermal correction is
explicitly approximate; no laboratory accuracy or known uniform error bar is
claimed. The law gives positive monotonic compression and joins deeper ice with
an unblended density jump. Both solid-side transitions are coarse structural
proxies, not matched Gibbs equilibria. Original source audits remain references,
not prerequisites for another research campaign.

Cold approximate regions expose human-readable approximation notes. Tests cover
the anchor, independent forward pressure closure, domain refusals, monotonicity,
the original Ih overlap and full cold-planet conservation/refinement. The solved
250 K fixture is documented in the solver guide. Its new cold treatment leaves
the 300 K acceptance result unchanged.

### Pressure-melted subfreezing liquid

Between **251.165 and 273.15 K**, the IAPWS Ih melting curve gives the lower
pressure boundary of liquid. The III branch through 256.164 K and V branch above
it give the high-pressure freezing boundary. Effective intermediate ice lies
above that, followed by the rounded 2.1 GPa deeper-ice boundary. Where Ih extends
beyond 100 MPa, its named pressure extension ends on the Ih melting curve rather
than on a fixed 220 MPa boundary. Source triple-point rounding remains an
approximation, not a new exact phase-equilibrium claim.

`approximate_subfreezing_compressed_liquid_v1` supplies otherwise unavailable
liquid density. Its reference is direct IF97 liquid at **100 MPa/273.15 K**:
`rho(P) = rho_ref [1 + 5(P - 100 MPa)/(2.8 GPa)]^(1/5)`.
The precomputed reference density is **1045.2740171091934 kg/m³**, checked against
direct IF97 in tests, so no uncharged nested EOS call is hidden in runtime evaluation.
The **2.8 GPa** reference bulk modulus and pressure derivative **5** are explicit
engineering assumptions of plausible liquid-water stiffness, not measured
constants from an attributed fit. Temperature dependence is omitted, so this is
not an implementation of supercooled IAPWS95 or a known-accuracy thermal EOS.
It is used only between the named melting curves, including explicit one-sided
limits, and also bridges compressed liquid at 273.15–273.16 K where the original
dense-liquid temperature mask starts. Original IF97 density remains in use in
its supported shallow domain, and original IAPWS95 remains in use above 100 MPa
where its temperature domain permits.

This fills the former radial cold/subfreezing coverage gaps for the prescribed
Earth-mass fixture across **250–300 K**. Tests include cold/liquid and III/V/VI
temperature seams, independent shell mass/inertia/hydrostatic closure, and
fourfold refinement at 260 K. The approximate prescriptions can have small
unforced model handoffs; they do not supply continuous thermal evolution,
latent heat, salinity, or precise intermediate polymorphs. There are no density
offsets or pressure repairs to force models to agree. Local `water::state` and
`dense_liquid::state` retain their original domains and coexistence refusals.

## Thermal assumptions and earlier integration gates

This module takes **local prescribed temperature**, not irradiation or equilibrium
temperature. It supplies no climate, greenhouse, convection, entropy or cooling
model. Refractory temperatures and phases remain unresolved in the cold solver.

The first restricted water-bearing radial API prescribes an **isothermal
pure-water region** and an absolute outer pressure. This is a conditional static
benchmark, not a prediction of planetary heat transport. Refractories retain
their named cold density proxies. External pressure initially represents a load
with no hidden atmospheric mass; inventory-backed atmosphere matching remains
checkpoint 4.

The restricted solver reuses the bounded kernel, conserves water allocations,
preserves one-sided composition-interface samples and tests independent shell
mass/inertia/hydrostatic closure, loading and trace-water refinement. It requires
a 0.01 Pa outer residual and a central-pressure-independent local pressure scale,
rather than the dry solver's `max(1 Pa, 10⁻⁸ P_c)` residual. Supported ice-Ih/liquid
crossings are now located with disjoint phase-region allocations and separate
endpoint densities. Their common density coverage is only 273.15–273.16 K,
restricted further by the 1,000 Pa floor. Colder stable liquid remains unsupported.
See the [solver guide](planet-composition.md#headless-restricted-water-shell-solver)
for exact scope and numerical checks.

The following were the earlier research-complete gates, superseded for game
checkpoint 3 by the effective-ice scope above. They remain relevant only to a
future resolved-polymorph model. The solver already reused the bounded kernel and independently tested local
ice-Ih/liquid interfaces. Broader water-rich radial requests still require:

1. Validated density coverage and equilibrium matching for deeper/cooler phases,
   rather than widening the current masks or joining unmatched fits.
2. Located interfaces and independent closure/refinement tests for every added
   branch, preserving one-sided densities and disjoint allocations.
3. Continued refusal of unavailable branches and out-of-domain water layers.
   A single supported outer state does not certify its deeper structure.

The one-Earth-mass, 30%-water fixtures in the
[structure contract](../plans/planet-structure.md#example-requests-and-expected-coverage)
now have solved approximate 300 K and 250 K results. They are not validated
shallow-water worlds or a resolved cold phase diagram.

## Candidate evaluation and provenance

### IAPWS

Equations, coefficients and reference values are attributed to the **International
Association for the Properties of Water and Steam**. All four release cover pages
permit publication in whole or in part in all countries with attribution. This
implementation transcribes those mathematical formulations, not third-party
software. No new runtime dependency, external source code or tabulated dataset is
imported. Interpolation validation is therefore not applicable to this baseline.

Pinned source releases and SHA-256 of the PDFs consulted:

| Release | Source | SHA-256 |
| --- | --- | --- |
| R7-97(2012) | [IF97 release](https://iapws.org/technical-guidance/release/IF97-Rev) | `c92f887e989cbf074af1fa982083dc54195d57691eab4fbc950ef6098d4cf1f4` |
| R10-06(2009) | [Ice-Ih release](https://iapws.org/technical-guidance/release/Ice-2009) | `d1cc6886c423b257576fbd5ef546cae9896b62230915e17ff26fa059cfe4f127` |
| R14-08(2011) | [Melting/sublimation release](https://iapws.org/technical-guidance/release/MeltSub) | `706b17a9c646979aa77c589db145a6a707e7b19527aefbdb70dc230898cfc839` |
| R6-95(2018) | [IAPWS95 release](https://iapws.org/technical-guidance/release/IAPWS-95) | `512879217b94f4d0741c88cab743d41098268914d538d73e680a736cb1c3aad7` |

IF97 remains the original fast low-pressure liquid baseline. The separate direct
IAPWS95 API now supplies compressed-liquid densities to 1 GPa within its own mask.
Neither formulation by itself supplies high-pressure ice densities or solid-solid
interfaces; neither API claims a thermal solution or deep-water planet structure.

### AQUA

Evaluated repository revision:
[`e3a234e54af1091eea8ca56a31328ef3e3b00e33`](https://github.com/mnijh/AQUA/tree/e3a234e54af1091eea8ca56a31328ef3e3b00e33),
MIT licence, Haldemann et al. (2020). It combines ice, liquid, gas and dense-water
models over a much wider domain. Its P–T interface uses Pa and K and returns
density in kg/m³, plus thermodynamic quantities. It is not selected/imported yet:

- [Issue 4](https://github.com/mnijh/AQUA/issues/4) reports incorrect Ma19 free
  energy/entropy values inherited from that component's original release, and
  links corrected source data. It remains open in the evaluated repository.
  The report distinguishes affected values from derivatives, including the
  adiabatic gradient; this is not evidence that all AQUA densities are invalid.
  Corrected entropy values, conventions and cross-model matching must be verified
  before entropy-anchored or adiabatic planetary use.
- [Issue 3](https://github.com/mnijh/AQUA/issues/3) and the README document
  inadequate density-grid spacing near 1,000 kg/m³ in the density–temperature and
  density–energy tables. Do not infer that the P–T table has exactly that same
  inversion defect; its interpolation and phase boundaries still need their own
  validation against direct source equations.
- A blended EOS/source-domain identifier is not automatically a stable phase
  identifier. High-pressure phase selection, masks and one-sided density
  interpolation require separate verification. The low-pressure IAPWS baseline
  now supplies direct-equation reference checks for future overlapping tables.

### SeaFreeze: evaluated reference, not an imported EOS

The gates in this historical candidate evaluation are not checkpoint-3 blockers:
the implemented game model uses the separate effective-ice approximation above.

Evaluated [SeaFreeze v1.1.3 source revision
`f6cd2bc8d9fca076ed093375248937b58957d582`](https://github.com/Bjournaux/SeaFreeze/tree/f6cd2bc8d9fca076ed093375248937b58957d582).
The repository and supplied spline assets are distributed under **GPL-3**, not
MIT. No separate permissive permission was established for the coefficients.
Do not copy source, splines or derived interpolation tables into this MIT project
without resolving distribution terms. Local reference-only evaluation under
ignored `data/water-sources/` does not add a runtime or shipped-data dependency.

Its Gibbs-local-basis models offer a useful matched candidate set: Journaux
et al. (2020) ices II/III/V/VI and Bollengier et al. (2019) liquid (`water1`),
plus Feistel/Wagner Ih and French/Redmer VII/X. The authors explicitly require
`water1` for equilibrium with their ice models; substituting `water2` or
`water_IAPWS95` Gibbs energies is not an approved phase-matching shortcut.

**Unit and domain audit:** pressure input is **MPa**, temperature K, Gibbs energy
J/kg and density kg/m³. A future SI adapter must convert pressure by 10⁶.
Loaded spline knot extents include VI: 0–3,000 MPa, 0–400 K; VII/X:
1,700–1,000,000 MPa, 20–1,800 K; `water1`: approximately 0–2,300.6 MPa,
239–501 K. These are mathematical representation extents, **not stable-phase
validity bounds**. In particular the broad IAPWS95 spline extent does not extend
IAPWS95's endorsed scientific domain. The reference `whichphase` minimises over
available finite Gibbs energies; losing an out-of-range competitor is not proof
that the remaining phase is stable. A future selector must validate competitor
coverage and coexistence explicitly rather than accept every returned label.

Local probes, without extrapolation or Gibbs offsets:

- All three Ih published density values agree within relative 9×10⁻¹².
- `water1` differs from the three IF97 verification densities by relative
  +1.25×10⁻⁶, −8.87×10⁻⁶ and −3.49×10⁻⁷. These are different liquid models,
  not a Rust transcription comparison. The documented VI state at 900 MPa,
  255 K gives 1,356.07249 kg/m³, consistent with its printed 1,356.1 value.
- Solving `G_ice = G_water1` at the IAPWS table-3 temperatures gives pressure
  differences of −0.185% (Ih), +3.521% (III), −0.174% (V) and −0.330% (VI).
  The III difference exceeds IAPWS's quoted 3% pressure uncertainty alone;
  this requires experimental/source-model investigation, not automatic rejection
  of every SeaFreeze density or an assertion of combined statistical significance.
- Pairwise Gibbs crossings at 250 K are approximately 219.94 MPa (Ih/II),
  351.38 MPa (II/V) and 664.51 MPa (V/VI). At 300 K, liquid/VI crosses near
  993.80 MPa. These are candidate equalities, not validated stable interfaces.
- **VI/VII matching gate:** at both 250 K and 300 K the direct VI and VII/X
  Gibbs values do not bracket equality over their shared 1,700–3,000 MPa
  interval. A 1 MPa-spaced scan (1,301 points) finds `G_VI - G_VII/X` negative
  throughout: −59.14 to −21.02 kJ/kg at 250 K and −40.61 to −4.58 kJ/kg at
  300 K. This is a reproducible seam/domain concern for these prescribed paths,
  not proof of an error in every component or an experimentally validated
  transition elsewhere. Do not create a boundary at a spline cutoff, extrapolate
  VI or add an arbitrary energy offset to conceal it.

Source asset SHA-256 values for reproducing the seam probe:

| Asset under `Python/seafreeze/splines/` | SHA-256 |
| --- | --- |
| `ice_VI/ice_VI.mat` | `223620c3096066f00dda5bea04f6af50be7043da3668ea28b927751441d9819d` |
| `ice_VII_X_French/ice_VII_X_French.mat` | `e7f99d83bf3c09d7ff7bfe7afba73cae2136133903bdaabc1410aa79405bd030` |
| `water_Bollengier/water_Bollengier.mat` | `26b21b6652ab4e292b4da747ca2e5877cd21b213a57f9ea31b9983643a678ee6` |

Local investigation script/results: `data/water-sources/seafreeze-probe.py` and
`seafreeze-probe.json`; all downloaded reference-file hashes are recorded beside
the pinned checkout. Evaluation used Python 3.14, NumPy 2.5.4, SciPy 1.18.1,
hdf5storage 0.2.2, h5py 3.16.0 and psutil 7.2.2 in a project-local reference venv.
These ignored research artifacts are not portable project tests or shipped data.

**Selection outcome:** IAPWS R14 is selected for independent melting-curve
references, direct IAPWS95 for bounded compressed-liquid density, and the separate
Lai 300 K fit for prescribed ice-VII compression. No complete
high-pressure ice-density/solid-solid package is approved for runtime yet.
Before deep-water radial integration, resolve a distributable ice-density source and
physically supported VI/deeper-solid matching on the 250/300 K paths, including
the applicability of omitted ordered ice phases. Then validate density and
equilibrium against primary references independently of any table generator.

### Primary deep-ice checks: isotope and thermal-model gates

[Klotz et al. (2017), Phys. Rev. B 95, 174111](https://doi.org/10.1103/PhysRevB.95.174111)
provide measured VII compression at 298 K and VIII compression at 93/196 K.
The samples are **deuterated water (D₂O, 99.6% D)**. Those volumes and fitted
parameters must not silently become H₂O density references by changing only
the molar mass, and 250 K is not one of their VIII isotherms. Their finding
of nearly equal low-pressure VII/VIII compression does not establish equal
H₂O Gibbs energies or permit labelling the VII branch as equilibrium VIII.
Consulted [HAL PDF](https://hal.sorbonne-universite.fr/hal-01677315v1/document):
SHA-256 `bd2fa52d81d18e72d450679f0fba45a0c4558e162f96d23d758614f1afbe23fc`.

The primary [Journaux et al. (2020) paper](https://doi.org/10.1029/2019JE006176)
also makes clear why copying its table-3 cold compression parameters alone
would not reproduce its finite-temperature ice densities: equations 4–9 add
thermal pressure from volume-dependent, mode-resolved phonon densities of
states. The four mode families have different Grüneisen parameters. No
arbitrary single Debye temperature, omitted thermal pressure or replacement
spectrum is accepted as that published model at 250/300 K. The phase Gibbs
reference values were matched against Bollengier liquid at specified triple
points, not against an arbitrary zero of IAPWS95. Consulted
[published PDF](https://www.dora.lib4ri.ch/empa/dload/empa:21683/PDF/Journaux-2020-Holistic_approach_for_studying_planetary_hydrospheres-(published_version).pdf):
SHA-256 `32be1644a63fa452a5fcd273d893185a065243577d0084ac7b0df2197615deff`.
This does not resolve distribution of the full spline/spectral assets.

[PALEOS v1 (2026), section 3.3](https://arxiv.org/html/2605.03741v1#S3.SS3),
was also checked as a potential alternative. Its water route interpolates AQUA,
with an entropy/internal-energy correction, rather than providing an independent
VI/VII/VIII density and equilibrium construction. Its broad planetary results
therefore do not remove the interface and cold-phase gates identified here.

### Independent ice-VI density reference: isotope metadata must be audited

[Chodkiewicz et al. (2022), IUCrJ 9, 573–579](https://doi.org/10.1107/S2052252522006662)
provide an independent, later H₂O ice-VI lattice measurement, separate from the
Bezacier (2014) compression study. The article is **CC-BY-4.0**, as confirmed by
the [PMC record](https://pmc.ncbi.nlm.nih.gov/articles/PMC9438488/) and its licence
metadata. The deposited supplementary CIF and supporting PDF are pinned below;
neither file is shipped in the runtime.

The usable H₂O observation is at **1.5(1) GPa** (supporting table 1Sa) and
**293(2) K** (CIF), with **Z=10** molecules and **V=215.818(19) Å³**. Using
`M=0.01801528 kg/mol` and exact SI Avogadro's constant gives
**1,386.12517 kg/m³**. Propagating only the printed volume standard uncertainty
gives 0.12203 kg/m³; this is **not** the total uncertainty of a P–T EOS comparison.
Pressure/temperature uncertainty and experimental systematics must also be
considered. In particular this is not a 300 K density, and no unvalidated thermal
correction is applied to make it one.

The source audit found several traps that prohibit treating every deposited
record as an independent H₂O density reference:

- All eleven named blocks use formula `H2 O` and rounded densities calculated
  with H₂O mass, even the seven D₂O and two mixed-isotope blocks. Isotope identity
  must come from the publication's sample mapping, not those formula/density
  fields. H₂O-equivalent numbers from excluded samples are not physical D₂O
  densities or independent H₂O checks.
- `H2O_home_HAR` and `H2O_home_IAM` refine the same measurement and cell. They
  contribute **one observation**, not two independent validation points.
- An embedded `data_woda_IIIb` inside a semicolon-delimited refinement text field
  is not another experiment. A naive line-based data-block split would count it.
- One lattice observation supplies neither a pressure-dependent density EOS nor
  liquid/VI or VI/VII equilibrium matching. It does not unblock the 30%-water
  fixture, certify a whole compression curve, or resolve ice VIII at 250 K.

Pinned artifacts from the article's separately downloaded supplements:

| Artifact | SHA-256 |
| --- | --- |
| `lt5051sup1.cif` (PMC `m-09-00573-sup1.cif`, 910,416 bytes) | `738b41d8d101c5d46f138fd8124eb829440f7397227adcfde0dd7325e01ebe5a` |
| `lt5051sup2.pdf` (PMC `m-09-00573-sup2.pdf`, 848,169 bytes) | `813c08809c7771f7d37ae654a82971e30a2c2642c1d3603ac7c659039b0a9d1c` |

The [Europe PMC supplementary-files endpoint](https://www.ebi.ac.uk/europepmc/webservices/rest/PMC9438488/supplementaryFiles)
provides both files in a ZIP. The audit is offline, standard-library-only and
requires the exact hashes before parsing. Pressure and its printed uncertainty
are explicitly transcribed from the pinned supporting PDF, not inferred from the
CIF volume. It reports sample identities, duplicate refinements and measurement
limitations without fitting an EOS or importing source assets:

```sh
python scripts/audit-ice-vi-reference.py data/water-sources/Chodkiewicz-2022-m-09-00573-sup1.cif data/water-sources/Chodkiewicz-2022-m-09-00573-sup2.pdf
python scripts/test-ice-vi-reference.py
```

Five offline tests cover uncertainty precision, embedded text, malformed/duplicate
fields, hash refusal, isotope exclusions, deduplication, unit conversion and
lattice consistency. Source-specific parsing is intentionally narrow, not a
general-purpose CIF reader. The audit result remains an independent reference
preparation step, **not high-pressure model validation or new runtime coverage**.

A separate local reference-only probe evaluates the already pinned SeaFreeze VI
model at the observation's actual P–T, not at 300 K. It returns
1,390.64517 kg/m³ at 1.5 GPa/293 K, **+0.32609%** relative to the measured-cell
density. Nine diagnostic probes at 1.4/1.5/1.6 GPa and 291/293/295 K span
1,383.02637–1,398.06118 kg/m³. This illustrates why the volume-only uncertainty
cannot serve as an EOS acceptance threshold. The nine probes are neither a
probability propagation nor a statistical agreement test, and one observation
does not validate a whole curve. They remove neither SeaFreeze's distribution
gate nor the independently identified VI/VII Gibbs seam. Reproduction with the
existing local reference environment is in ignored
`data/water-sources/ice-vi-independent-probe.py`; its JSON/log are diagnostic
artifacts, not shipped EOS data or portable project tests.

### Revised AQUA (2026): distributable candidate, not interface-ready

The original AQUA repository issue is not the latest scientific status.
[Cano Amoros et al. (2026), A&A 708, A324](https://doi.org/10.1051/0004-6361/202557580)
publish revised AQUA with corrected Mazevet entropy, new superionic data and
revised density/internal-energy interpolation. The paper and the pinned
[Zenodo record 18609452](https://doi.org/10.5281/zenodo.18609452) explicitly
use **CC-BY-4.0**. This provides a separately licensed candidate dataset, not
permission to relicense SeaFreeze source or assets as MIT. Any eventual shipped
data needs attribution, the licence notice and an account of modifications.
The bundled README's licence field is blank; the licence above comes from the
record metadata, not an invented README permission.

Pinned downloaded artifacts:

| Artifact | SHA-256 |
| --- | --- |
| `AQUA_revised_eos_pt.csv` (37,263,742 bytes) | `baaf15a2108c0690f3e8d544174d7cc923376fe0de52dee4a277d9b14675bdbf` |
| `_README.txt` | `debb7adce0c78e1a950fc3399333d0ea1810aaad48bc1b471ebcfcdc8f5b6ae6` |
| Paper PDF, [DLR copy](https://elib.dlr.de/224800/1/aa57580-25.pdf) | `03da987f3daff19d422933dfec34691988a58381583ef02549b60207cb4ed307` |

The CSV has 303,854 rows on a rectangular 1,093-pressure × 278-temperature
grid, spanning 0.1–4×10¹⁴ Pa and 100–58,884.3655 K. Input axes and density
are finite and positive. Pressure, density, energy and entropy units are SI.
The table distinguishes named ice phases, vapour and liquid, but phase 5
combines supercritical and superionic states; phase 0 is extrapolated ice.
Those are not sufficient phase identities for every requested planet region.
Entropy `−1` is an invalid/cut sentinel, not a thermodynamic value.

The supplied flags are 0 (original), 1 (interpolated), 2 (extrapolated) and
3 (not applicable). Counts are 211,435, 25,290, 40,935 and 26,194 respectively.
An original flag alone does not establish scientific validity or prevent
interpolation across a phase jump.

The interface gate remains important:

- Neither 250 K nor 300 K is a table temperature. Their neighbouring columns
  are 245.470892/251.188643 K and 295.120923/301.995172 K. Across these pairs,
  35 and 22 rectangular cells respectively have mixed phase labels at their
  corners. Naive bilinear interpolation cannot preserve one-sided densities.
- At 301.995172 K, flagged interpolation starts near 592.54 MPa and extends
  across the stored liquid/VI label change near 1,036.59 MPa. At that labelled
  VI point the density is 1,235.69 kg/m³, then becomes 1,359.04 kg/m³ at the
  first unflagged VI node near 1,107.09 MPa. The paper explicitly interpolates
  density and energy wherever it interpolates entropy. A label is therefore
  **not proof of an unblended pure-phase density** at an interface.
- The paper (section 3.4) acknowledges that cold water would structurally
  change to ice VIII, whereas the supplied region-3 model treats VII/X. The
  entropy correction does not by itself resolve the cold solid-phase gap.

This audit establishes provenance and concrete masking/interpolation concerns,
not full accuracy or invalidity of the dataset. No table, third-party software
or derived EOS grid is imported into the runtime. Direct IAPWS95 now supplies
an independent fluid reference for future comparisons. Corrected AQUA remains
a candidate for wider regimes, not an enabled fallback for unavailable ice.

Reproduce the structural audit after separately downloading the pinned CSV:

```sh
python scripts/audit-revised-aqua.py data/water-sources/revised-aqua-18609452/AQUA_revised_eos_pt.csv
```

This tracked, standard-library-only script checks the file hash, schema, axes,
positive finite density, flags, neighbouring columns and mixed-phase cells.
It neither downloads data nor generates an interpolated runtime EOS. Local
source artifacts and the audit JSON remain under ignored `data/water-sources/`.

## Reproducing validation

```sh
cargo test -p space-simulation system::materials::water
```

Twenty-four tests cover the local models, including the effective-ice and explicit
liquid-limit checks described above. The original six tests cover published
IF97 specific volumes (all three table-5 states), all
three ice-Ih table-6 densities, saturation table 35, the melting-curve reference
and endpoints, pressure-dependent freezing, coexistence refusal, unsupported
subfreezing liquid/vapour, application-mask edges, invalid/nonfinite inputs,
deterministic finite states and positive continuous single-phase compression.
Two ice verification points are outside the public application mask; only the
private equation is exercised there, without enabling those states publicly.
The four melting-module tests additionally cover all five table-3 melting
pressures, triple-point endpoint rounding, each branch's domain and monotonicity,
published uncertainty metadata, deterministic results and continued refusal of
unsupported high-pressure densities/phases. No external Python package or
network access is required by the Rust tests.

One additional local test checks named Ih/liquid endpoint densities against their
one-sided limits and preserves coexistence-mixture, metastable, subfreezing-liquid,
vapour-interface and pressure-domain refusals. Radial phase-interface acceptance
is documented in the [solver guide](planet-composition.md#headless-restricted-water-shell-solver).

Five compressed-liquid tests check the printed IAPWS95 table-6 residual and
derivative and eleven table-7 pressures (including private equation checks outside
the public mask), two public high-pressure density inversions, an independent
five-point derivative stencil, deterministic positive compression and pressure
closure across the application grid. They also test melting/coexistence refusal,
invalid/nonfinite domains, every insufficient inversion budget with exact consumed
work, inversion exhaustion and the unforced IF97 overlap.
The table-7 pressure comparison tolerance is relative 10⁻⁸ and public density
comparison tolerance 2×10⁻⁹ against the printed verification states. These
are numerical verification targets, not scientific error bars.

Four ice-VII branch tests check three independent 55-digit Decimal evaluations
of the BM3 polynomial, the two printed H₂O lattice observations with their actual
fit deviations, deterministic positive compression and independent Eulerian-strain
pressure closure across 166 pressures. They exercise invalid/out-of-mask inputs,
250 K refusal, every insufficient budget up to a representative solve, exact
consumed-work accounting and unchanged equilibrium/liquid API refusals. All Rust
tests remain headless and require no reference download or Python environment.

Relative numerical targets against the **printed** reference values are 5×10⁻⁹
for IF97 volumes and saturation, 5×10⁻¹² for ice density and 4×10⁻⁶ for the
more coarsely printed melting pressures. These are transcription/evaluation
checks, not scientific error bars, radial convergence or water-world calibration.
