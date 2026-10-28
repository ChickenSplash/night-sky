# Physical atmospheres

`crates/simulation/src/system/atmosphere.rs` provides validated physical
states and a first column approximation, independent of Bevy and shaders.
`system/atmosphere/generation.rs` now assigns a physical environment to each
planet after the entire stellar hierarchy and stable orbits exist. The
inspector shows that data. `system/atmosphere/optics.rs` derives approximate
visible optics, which the client applies to each planet's atmosphere shell.
See [the plan](../plans/atmospheres.md#checkpoint-5--every-planet-with-air)
for checkpoint history and remaining visual validation.

Composition generation reconciles secondary escape, bulk H/He retention and
structure first on fast fallback facts, sharing this module's final-orbit all-star
heating samples. Flux, activity-weighted loss forcing, seeded albedo and age are
prepared as independent inputs before retention. Atmosphere output never feeds
back into that solve. Existing live atmosphere/geometry consumers still receive
a legacy compatibility projection afterwards, pending the P3 cutover.
Atmospheric supply and cold condensates use those same budgets. Re-evaluating
population at a different age can change retained mass, radius and spin, but never
formation inventories, IDs, orbits or resonances. See the
[composition model](planet-composition.md#formation-priors-and-retained-structure).

Staged synchronous results now also prepare their **own** source-bounded gas and
condensate partition at the fallback reference, with matching pressure,
temperature and updated inertia. This does not replace the live compatibility
atmosphere above. The [reference-partition approximation](planet-composition.md#synchronous-reference-partition)
holds proxy geometry fixed, retains trace gas and keeps bulk-envelope mass
distinct from the sampled optical column. It is ready for coherent adoption,
not permission to mix a candidate with the legacy live environment.

## State and reference level

Layered checkpoint 4 also supplies a separate headless
[condensed-boundary coupling API](planet-composition.md#headless-atmosphere-and-condensed-boundary-coupling).
It matches inventory, base pressure and radial structure within a restricted
spherical-column domain. It does not yet replace generation or consumer paths
described in this guide. Cold-trap caps are shared, but generation's accessibility
priors and weak-binding fallback are not adopted as solved interior physics.

An atmosphere is explicitly airless or contains a gas atmosphere. Absence
is not zero pressure or the renderer's minimum slider value. A gas mixture
stores mole fractions, not mass fractions, for N₂, CO₂, H₂, He, H₂O, CH₄,
O₂ and Ar. Supporting oxygen makes no claim about its natural abundance.
Each fraction must be finite and between zero and one; duplicate constituents
are rejected. Fractions must sum to one within 1e-9, with only that rounding
error normalised. No missing gas or arbitrary weight normalisation is inferred.

Pressure and temperature refer to a defined level:

- **Surface:** a condensed-boundary column, before terrain corrections. Water-rich
  boundaries have unresolved phase, not necessarily landable solid ground. Tiny
  H/He inventories that cannot support one bar also use this reference. Supported
   low-gas condensed worlds use their approximate boundary even above one
   bar, with the [bounded domain and handoff](planet-composition.md#supported-approximate-condensed-boundaries)
  determining reference semantics, never their descriptive class. Pressure
   can be any positive finite value.
    Extended geometry retains this boundary and its inventory-normalised pressure,
    not a switch to a groundless one-bar planet.
- **One bar:** a chosen reference level for envelope worlds. Pressure must
  equal `ONE_BAR_PA` (100,000 Pa). This is not solid ground, necessarily a
  visible cloud top, or a promise that every measured giant radius uses it.

The caller supplies GM in m³/s² and the radius of that level in metres.
For spheroids use the existing volume-equivalent mean radius; the column
currently ignores latitude, rotation and atmospheric self-gravity. The column does
not independently choose geometry. Generation resolves the environmental reference
first and gives the same final radius to geometry, column, optics and clearance.

The reference temperature is a representative **isothermal gas-column
temperature**, not an equilibrium-temperature calculation or a solved
surface climate. Generation currently estimates it from equilibrium temperature
with a 20 K floor; there is no internal-heat solution, and the
[grey greenhouse estimate](#grey-greenhouse-estimate) is used only by the opt-in
coupled structure, not by generation. All stored units
are SI, including pascals, kelvin, metres and kilograms.

## Derived column

For mole fractions xᵢ and molecular masses mᵢ in kg per molecule:

```text
mean molecular mass = Σ xᵢ mᵢ
reference gravity  = GM / R²
reference density  = pressure × mean molecular mass / (kB × temperature)
gas scale height   = kB × temperature / (mean molecular mass × gravity)
```

These define reference values, not constant gravity throughout the column.
`atmosphere/profile.rs` owns the central-field hydrostatic geometry and shell-mass
normalisation, separately from partition chemistry and optical conversion.
For `H=H₀` at radius `R`, the spherical isothermal profile is:

```text
g(h) = GM / (R+h)²
ln[p(h)/p₀] = ln[ρ(h)/ρ₀] = -h / [H₀(1+h/R)]
h(z) = H₀ z / (1-z H₀/R), where z = ln(p₀/p)
```

Molecular masses use fixed representative isotope abundances; this is not a
chemical-equilibrium calculation.

One optional prescribed aerosol profile is separate from the gas mixture:
material, reference aerosol mass density, exponential scale height and a
representative particle radius. It is based at the same reference level,
with no detached cloud layer, size distribution, optical coefficients or
condensation solver yet. Its mass does not enter gas molecular mass/density.
Omit the layer for clear air rather than assigning it zero density.

Operational gas extent uses the potential drop
**`z_cut = 12.5 + max(ln(p₀ / 1 bar), 0)`**, converted to height by `h(z_cut)`.
A dense boundary column therefore ends at about 0.373 Pa; sub-bar columns retain
12.5 logarithmic density drops. Rebasing a spherical column at another pressure
level preserves the upper profile when its local gravity/scale height is recomputed.
Prescribed aerosol extent remains 12.5 aerosol scale heights; the larger extent
sets the rendered shell. This cutoff is neither an exobase nor a physical end.
`Column::density_at` can evaluate above it, but validity is not guaranteed there.
[Navigation clearance](craft.md#navigation-clearance) is calculated separately.

`GasAtmosphere::pressure_level_altitude_m` inverts this profile
for positive pressure levels at or above the reference. A deeper level returns
`None`, not a negative altitude or evidence that ground is absent. This is a
column diagnostic only, not an integration into an unresolved deep envelope.

An extent greater than 0.1 reference radii is flagged `Extended`, not clamped
or rejected. This is informational geometry, not a constant-gravity warning:
the spherical profile already accounts for weakening gravity.

An isothermal spherical profile tends to nonzero density at infinity, so it
cannot supply a finite total atmosphere without an operational cutoff or a
different outer model. Use it only when that cutoff lies below the isothermal
Parker sonic radius `r_s=GM/(2c_s²)`, equivalent to `R/H₀ > z_cut+2`.
This is an engineering guard, not a solved wind or proof of hydrostatic validity.
Otherwise retain the legacy exponential profile and planar inventory pressure,
explicitly flagged `hydrostatic_unresolved`. The inspector discloses unresolved
escape/thermal structure rather than silently hiding the limitation.
Invalid inputs and nonfinite/nonpositive derived values return errors.

The [composition census](planet-composition.md#calibration-census-and-regression-scope)
records how often generated columns are extended, including their distribution
across classes, hosts and ages. Extended columns are common, not a rare numerical
corner case. Passing optical conversion and conservative route-clearance checks
does not validate their density profile or the underlying escape prescription.

## Inventory-backed generation

Each atmospheric draw uses its planet ID plus `:atmosphere:v1`, independently
of the orbit/spin streams. Stellar system age is obtained from the same draw
as companion generation, using a cloned RNG so no existing draws are consumed.
The same albedo draw is now prepared before retention, not inside atmosphere
output generation; its seed and range are unchanged. Retention uses fallback
mass/radius facts, without an EOS or previously generated atmosphere. It keeps
the fixed-albedo exosphere heuristic, distinct from the drawn thermal input.
Retention updates physical bodies before the environmental reference correction
and partition. The correction can change radius/inertia/spin, never retained mass,
source/escape accounting or orbital elements.

1. **Heating from all stars.** Add bolometric `L / (4πd²)` for every star,
   including companions outside the planet's host. Average 32 deterministic
   phase samples through the nested stellar hierarchy. The planet's mean
   anomaly is stratified over its orbit; stellar anomalies use different
   low-discrepancy sequences. This is an approximate independent-phase average,
   not an initial-date snapshot, resonant/secular integration or an eclipse
   model. Clamp separations at stellar radii to avoid singular point-source
   fluxes. The phase budget is fixed and accuracy is not guaranteed in every
   close hierarchy.
2. **Thermal estimate.** Draw an assumed Bond albedo between 0.1 and 0.6, then
   use `T_eq = [F(1-A)/(4σ)]^(1/4)` for full redistribution and unit emissivity.
   This albedo is a provisional input, not derived from the renderer. Keep
   equilibrium temperature separate from the floored column estimate.
3. **Loss trends.** Use a smooth, shoreline-inspired secondary-material
   surviving mass fraction, falling with heating and rising with escape speed
   to the fourth power. Its nominal transition at Earth's escape speed is
   ten solar fluxes, an uncalibrated prescription, not a reproduced empirical
   fit or a guarantee of survival. Weight loss forcing by a saturation-then-
   inverse-age activity proxy: low-mass stars stay active longer, and hot stars
   receive an extra UV proxy factor. This does not calculate XUV spectra,
   flare events or the irradiation history of evolved stars.
4. **Conserved supply.** Class never selects gas occurrence or composition.
   Use retained H₂/He and the accessible retained secondary molecular supply,
   not a separate pressure or mixture draw. The [composition guide](planet-composition.md#secondary-escape-and-reservoir-partition)
   specifies exposure fractions, protected material, escape accounting and the
   bounded H/He solve. No additional atmospheric mass is invented.
5. **Partition.** Rough Clausius-Clapeyron limits cap cold CO₂, N₂, CH₄, Ar and
   water partial pressures; the remainder stays in `CondensedVolatiles`. This is
   cold trapping, not a solved phase diagram. Pressure and molecular fractions
   are solved consistently with the actual gas masses, not clipped independently.
    No aerosols or clouds are inferred from the transferred mass.
    Test the bounded thin-boundary domain and reconcile its geometric reference
    before exporting the final atmosphere. Unsupported deep envelopes retain a
    one-bar reference; no giant interior is inferred from a cold heavy proxy.
6. **Absence and columns.** Boundary pressure below 0.1 Pa is treated as
   absent/trace for this population model. This cutoff is not a physical ban
   on exospheres. Validate the physical state and derive its column. Retain
    geometric extent and unresolved-hydrostatic diagnostics in the snapshot,
    without silently shrinking it.

No oxygen, aerosols or cloud cover are generated yet. Supported materials in
the data model are not a claim that this recipe already predicts them.
Generating a gas mixture is insufficient evidence for aerosol loading, so
prescribed aerosols remain an extension point rather than random haze sliders.

### Pressure and cold-trap solve

At a condensed boundary, conserve the represented spherical shell mass:

```text
M_gas = (4πR²/g₀) p₀ F
F = ∫₀^z_cut exp(-z) / (1-z H₀/R)^4 dz
```

The Jacobian includes both weakening gravity and expanding shell area. Fixed
256-panel Simpson quadrature in potential-drop coordinates resolves thin and
extended columns without integrating over badly scaled physical radii; negligible
tails beyond 64 drops are omitted. The unresolved exponential fallback uses `F=1`
and explicitly retains legacy planar inventory pressure, not exact spherical mass.

With molecular masses `μ_i` in common units, set `p_i=C m_i/μ_i`.
For each trial `C`, allocate `m_i=min(supply_i, p_sat_i μ_i/C)`, then reconcile
`p₀ F` with inventory weight using the resulting mixture and temperature.
A fixed 64-step bisection uses zero and a molecular-mass-based upper bound.
After reservoir transfers, recompute pressure from the actual stored gas mass
and mixture, avoiding cancellation errors in trace allocations. Check residual
saturation caps with the declared 1e-9 relative roundoff allowance. This is a
well-mixed column, not independent species scale heights.

When structure supplies a one-bar reference, use the **entire** envelope reservoir
but solve `C Σ(m_i/μ_i)=100000 Pa` instead. The brackets use all offered molecules
and the noncondensing H₂/He alone. This caps condensable mole fractions at that
reference, not at a fictional envelope bottom. The H₂/He structure supply check
already guarantees at least a one-bar column. Most giant gas lies deeper than
the represented column and is never counted a second time.

No arbitrary pressure cap truncates the inventory. Very dense boundary columns
retain the declared ideal-gas limitations; no steam expansion or greenhouse
solution is invented. Validating wire conversion and optics use the same profile.
Flight clearance keeps its hard
exclusion and swept safety stop; its preferred wrapping margin now includes
atmospheric clearance height, avoiding a near-zero steering gap around thick air.
Pressure alone never reclassifies a planet.

## Grey greenhouse estimate

`atmosphere::greenhouse` (`grey_eddington_pressure_fit_greenhouse_v1`) is a
labelled estimate of surface warming, not radiative transfer or a climate model.

- **Surface temperature:** `T_s⁴ = T_eq⁴ (1 + 3τ/4)`, the Eddington grey
  radiative-equilibrium surface.
- **Optical depth:** `τ = P^0.2808 · (4.07 p_H₂O^0.4453 + 5.40 (p_CO₂^0.4453 +
  p_CH₄^0.4453))`, pressures in bar at the surface. The absorber exponent stands
  for band saturation, the total-pressure exponent for pressure broadening.
- **Calibration:** CO₂ is fitted to Mars (210 → 215 K), Venus (229 → 737 K) and
  Earth's CO₂ share of about 20%. Water, with clouds lumped in, takes the rest of
  Earth's 255 → 288 K, with its vapour saturated at 288 K. CH₄ uses the CO₂ fit
  with no separate physics; on Titan this gives about 103 K, the greenhouse-only
  value before its haze anti-greenhouse.
- **Transparent:** N₂, O₂, Ar and H/He, apart from broadening. No
  collision-induced absorption, so thick H₂ or N₂ columns are under-warmed.
- **Not included:** lapse rate (the column stays isothermal at `T_s`), clouds as
  a separate term, ice-albedo feedback, the radiation limit of a moist
  atmosphere, and day–night or latitude contrast. Albedo stays the one
  generation drew.

With saturated water the warming feeds back on itself. Earth-like air over an
ocean runs away near `T_eq ≈ 262 K`, close to the usual inner edge of the
habitable zone. This is a fitted outcome, not a validated threshold.

## Visible optical conversion

### Explicit renderer boundary contract

`atmosphere::optics::boundary::OpticalColumn` declares the renderer's inputs
independently of structural layers and navigation clearance:

- Reference radius, pressure, temperature, gravity and reference-level meaning.
- Named spherical-isothermal or unresolved-exponential density approximation,
  with the existing gas/aerosol scattering and absorption coefficients.
- An opaque lower **proxy** at the reference: either a condensed-boundary proxy
  (not proof of phase or landability) or a gas-reference proxy (not a located
  cloud deck). These are rendering assumptions, not optical-depth detections.
- A finite outer radius and its height above the reference, with the declared
  gas-density-drop and aerosol-scale-height cutoff policy. This preserves the
  [existing cutoff](#derived-column), not a new opacity-adaptive outer boundary
  or a guaranteed bound on omitted light.

The current client builds this contract solely from the live atmosphere, mass
and radius. A staged `Planet::initial_result` supplies none of those inputs.
This preserves existing numerical optics while the coordinated live cutover
is pending. The lower proxy does not certify terrain or navigation permission;
the existing bulk-envelope visual descent policy remains separate and unchanged.

`OpticalColumn::from_result` is a separate, checked preparation API for unified
results, **not adoption**. It validates transported invariants, checks pressure,
temperature, reference semantics and well-mixed gas-reservoir composition, then
uses the result's reference gravity (enclosed mass), not total-mass gravity.
It does not run structure, EOS, retention or partition solves. In particular,
a fallback record with condensed secondary proxies cannot borrow a secondary
atmosphere from a legacy planet. Suspended condensates are explicitly unsupported
by this entry point until local vapour and cloud optical inputs are defined;
bulk condensate mass must not silently become gas or cloud opacity.

Structural outer edges never choose the optical cutoff. Visibility along a ray
depends on integrated scattering/absorption, not a constituent's name, the one-bar
coordinate or whether the screen looks dark. The renderer already has an
[opaque-ray early stop](../client/rendering.md#atmospheres); the finite outer
cutoff bounds the represented column, a different operation. This contract does
not extend the validated profile to arbitrary giant-interior depths.

### Coefficients and limitations

Optics remain independent of generation and Bevy. `GasAtmosphere::optics`
derives a column and reference coefficients per metre. All gas coefficients
follow its selected density profile, while prescribed aerosols have their own
profile. The client performs radius scaling; it never fits debug slider presets
to a generated mixture.

- **Rayleigh:** `beta(lambda) = [p/(kT)] × Σ x_i sigma_i(550 nm)
  × (550 nm/lambda)^4`, using representative channels 680, 550 and 440 nm.
  Rounded dilute-gas cross sections account for differing gas refractivity
  and depolarisation, with dispersion reduced to the fourth-power law.
  Earth-like clear gas agrees with the existing reference coefficients within
  8%. This is not spectral RGB integration or a dense-gas dielectric model.
- **Visible absorption:** a separate gas-profile term uses methane's
  mole fraction and tunable effective colour-band cross sections
  `[2e-29, 4e-30, 0] m²/molecule`. These are deliberately rough appearance
  prescriptions, not measured line-by-line spectra, validated band means,
  or a temperature/pressure-dependent methane opacity calculation. Other
  supported gases are treated as visibly nonabsorbing in this version. No
  oxygen-derived ozone, CO₂ infrared absorption or water greenhouse absorption
  is invented. CO₂ alone therefore does not give an orange sky.
- **Prescribed aerosols:** particles with radius at least 0.5 µm use a simple
  geometric extinction efficiency `Q_ext = 2`, so mass loading gives
  `beta_ext = 3 Q_ext rho_aerosol / (4 rho_particle radius_particle)`.
  Fixed representative material densities, absorption shares, scattering
  colours and phase asymmetries are isolated in the conversion. Dust and
  organic haze colours are illustrative, not derived from optical constants.
  This is not a Mie solver, and even the lower end of the accepted size range
  is only approximate. Smaller particles return an explicit unsupported-size
  error, rather than applying geometric optics to Rayleigh-sized particles.
  Generation still creates no aerosol loading or cloud deck.
- **Validity:** supported extended columns use spherical gravity in both CPU
  tables and WGSL. Unresolved cases retain their explicitly flagged exponential
  fallback, not an artificial radius or pressure reduction.
  Ideal-gas and dilute optical approximations remain unreliable for very
  dense, dissociating or ionised gas. Invalid/nonfinite conversions are rejected;
  input validation is not proof of accuracy in these regimes.

Reference-world tests use prescribed physical Earth, Mars, Venus, Titan and
hydrogen-rich inputs. They check clear-gas coefficients and physical scaling,
not equality to the old hand-tuned haze presets. Those presets include clouds,
dust or ozone that cannot be inferred from bulk gas composition alone.

For bulk H/He envelopes the renderer allows free-camera descent below the one-bar reference using an
explicit [visual continuation into darkness](../client/rendering.md#giant-descent).
This changes no simulation radius, column diagnostic or collision rule. There is
no pressure-imposed camera floor; physically resolved deep-envelope descent remains
future work. In particular, negative heights are not added to the validated
pressure-level diagnostic by this rendering approximation.
One-bar pressure semantics alone never enable this descent: unresolved
water-rich/trace-gas references and reference-handoff worlds are not proof of a
groundless bulk envelope.

## Transport and inspection

`Hosted::environment` is copied to `Planet::environment` for both normal
generated systems and task-overlay pages. `PlanetEnvironment` includes mean
irradiation, assumed albedo, equilibrium temperature and system age. Its
optional `AtmosphereData` carries physical inputs plus derived column facts.
Missing `environment` in older snapshots means **not generated**, whereas a
present environment with no atmosphere means **airless/trace**.

Physical consumers reconstruct `GasAtmosphere` through its validating wire
conversion and recompute columns from GM/radius. Do not trust cached derived
snapshot fields as physical inputs. The inspector reads generated facts, not
the debug override, and labels the 1 bar level as non-ground. Its developer
readout exports the same rows. Atmosphere search predicates remain future work.
Boundary columns show approximate boundary pressure and the isothermal altitude of
one bar (or that it is not above a sub-bar boundary). Shared composition facts flag
unresolved deep boundaries. All displayed gas columns disclose ideal-gas/dilute-optics,
dense-gas, greenhouse and phase limitations, independent of pressure or class.

## Scope and checks

Tests cover Earth-, Mars- and Jupiter-like example columns, gas-mixture
validation, ideal-gas/hydrostatic scaling, reference levels, aerosol extent
and hydrostatic fallback diagnostics. Independent radial quadrature checks shell
mass; derivative and inversion tests check the spherical profile. Generation tests cover isolated-star flux,
eccentric-orbit averages, stellar and circumbinary hosts, nested companions,
initial-phase independence, activity/retention trends, cold condensation,
wire validation and deterministic population diversity. Inspector tests cover
unknown, airless, surface and envelope records.

Inventory integration tests also check per-species source/retained/escaped sums,
gas/condensate conservation, saturation caps, mass-to-mole conversion, sub-bar
H₂/He, giant column supply, label independence and non-compounding re-evaluation.
Route tests use the calculated safe launch height, not a fixed altitude that
can lie inside newly generated air; the clearance refusal policy is unchanged.

Not modelled: physical escape rates, atmospheric evolution, chemical equilibrium,
resolved phase diagrams/hydrospheres, radiative transfer (beyond the grey
greenhouse estimate), nonideal dense gases,
dissociation/ionisation, upper-air
temperature structure, winds or resolved deep envelopes. Physically
valid input ranges are not evidence that these approximations hold in every
regime. In particular, dense/hot atmospheres and deep giant envelopes will
need better treatment. Optical conversion remains separate:
visible absorption is not greenhouse absorption, and CO₂ does not by itself
make a sky orange.
