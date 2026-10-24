# Physical atmospheres

`crates/simulation/src/system/atmosphere.rs` provides validated physical
states and a first column approximation, independent of Bevy and shaders.
`system/atmosphere/generation.rs` now assigns a physical environment to each
planet after the entire stellar hierarchy and stable orbits exist. The
inspector shows that data. **Rendering is not connected yet:** the client
still uses its hand-set atmosphere on the selected planet.
See [the plan](../plans/atmospheres.md#checkpoint-5--every-planet-with-air)
for the remaining optical conversion and rendering integration work.

## State and reference level

An atmosphere is explicitly airless or contains a gas atmosphere. Absence
is not zero pressure or the renderer's minimum slider value. A gas mixture
stores mole fractions, not mass fractions, for N₂, CO₂, H₂, He, H₂O, CH₄,
O₂ and Ar. Supporting oxygen makes no claim about its natural abundance.
Each fraction must be finite and between zero and one; duplicate constituents
are rejected. Fractions must sum to one within 1e-9, with only that rounding
error normalised. No missing gas or arbitrary weight normalisation is inferred.

Pressure and temperature refer to a defined level:

- **Surface:** the reference ground spheroid on rocky/dwarf worlds, before
  terrain corrections. Pressure can be any positive finite value.
- **One bar:** a chosen reference level for envelope worlds. Pressure must
  equal `ONE_BAR_PA` (100,000 Pa). This is not solid ground, necessarily a
  visible cloud top, or a promise that every measured giant radius uses it.

The caller supplies GM in m³/s² and the radius of that level in metres.
For spheroids use the existing volume-equivalent mean radius; the column
currently ignores latitude, rotation and atmospheric self-gravity. The new
convention does not change generated radii, geometry or collision behaviour.

The reference temperature is a representative **isothermal gas-column
temperature**, not an equilibrium-temperature calculation or a solved
surface climate. Generation currently estimates it from equilibrium temperature
with a 20 K floor; there is no greenhouse or internal-heat solution. All stored units
are SI, including pascals, kelvin, metres and kilograms.

## Derived column

For mole fractions xᵢ and molecular masses mᵢ in kg per molecule:

```text
mean molecular mass = Σ xᵢ mᵢ
reference gravity  = GM / R²
reference density  = pressure × mean molecular mass / (kB × temperature)
gas scale height   = kB × temperature / (mean molecular mass × gravity)
```

These follow the ideal-gas law and isothermal hydrostatic balance with
constant gravity. They imply gas pressure and density proportional to
exp(-height / scale height). Molecular masses use fixed representative
isotope abundances; this is not a chemical-equilibrium calculation.

One optional prescribed aerosol profile is separate from the gas mixture:
material, reference aerosol mass density, exponential scale height and a
representative particle radius. It is based at the same reference level,
with no detached cloud layer, size distribution, optical coefficients or
condensation solver yet. Its mass does not enter gas molecular mass/density.
Omit the layer for clear air rather than assigning it zero density.

Operational extent is **12.5 times the larger of gas and aerosol scale
heights**, above the reference level. It matches the renderer's exponential
cutoff convention, but is not connected to rendering or flight clearance yet.
It is not an exobase, escape boundary or physical end of the atmosphere.

An extent greater than 0.1 reference radii is flagged `Extended`, not clamped
or rejected. This is an explicit engineering threshold: gravity has already
fallen about 17% at that height. The returned numbers remain constant-gravity
estimates; future consumers must decide how to handle the warning. `Thin`
only passes this geometry check, not every possible physical validity check.
Invalid inputs and nonfinite/nonpositive derived values return errors.

## Initial generation recipe

Each atmospheric draw uses its planet ID plus `:atmosphere:v1`, independently
of the orbit/spin streams. Stellar system age is obtained from the same draw
as companion generation, using a cloned RNG so no existing draws are consumed.
Atmosphere work changes no masses, radii, classes, spins or orbital elements.

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
3. **Loss trends.** Use a smooth, shoreline-inspired secondary-atmosphere
   survival probability, falling with heating and rising with escape speed
   to the fourth power. Its nominal transition at Earth's escape speed is
   ten solar fluxes, an uncalibrated prescription, not a reproduced empirical
   fit or a guarantee of survival. Weight loss forcing by a saturation-then-
   inverse-age activity proxy: low-mass stars stay active longer, and hot stars
   receive an extra UV proxy factor. This does not calculate XUV spectra,
   flare events or the irradiation history of evolved stars.
4. **Provisional inventories.** Class only supplies a probability of an initial
   H₂/He envelope (0.99 gas giant, 0.95 ice giant, 0.8 sub-Neptune, 0.03 rocky,
   0.005 dwarf), multiplied by a soft hydrogen-retention trend based on a
   Jeans parameter and estimated exospheric temperature. No class is forced
   to have/lose air. Successful envelopes get H₂/He plus temperature-dependent
   trace methane, referenced at 1 bar. This is not an envelope-mass calculation.
5. **Secondary air.** Otherwise, survival and a broad volatile-supply draw
   determine a N₂/CO₂ mixture with Ar, CH₄ and H₂O. Supply pressure spans
   1 Pa to 10 MPa before mass scaling and loss, with an upper supply cap of
   100 MPa. Gas fractions share a single pressure budget, rather than being
   independent slider values. Rough Clausius-Clapeyron limits remove excess
   cold CO₂, N₂, CH₄, Ar and water vapour, then total pressure and mole fractions
   are recomputed. Methane in hydrogen envelopes is cold-capped too. Removed
   condensate is not yet assigned to an ocean, ice or interior reservoir.
6. **Absence and columns.** Secondary pressure below 0.1 Pa is treated as
   absent/trace for this population model. This cutoff is not a physical ban
   on exospheres. Validate the physical state and derive its column. Retain
   extended-column warnings in the snapshot, without silently shrinking it.

No oxygen, aerosols or cloud cover are generated yet. Supported materials in
the data model are not a claim that this recipe already predicts them.
Generating a gas mixture is insufficient evidence for aerosol loading, so
prescribed aerosols remain an extension point rather than random haze sliders.

These priors and formulas are intentionally collected in the atmosphere
generator. Later bulk-composition work can supply inventories and replace
class priors while keeping physical validation, wire conversion and optics.
Until then, class-derived radii are not self-consistent with retained envelope
mass or composition. Pressure alone never reclassifies a planet.

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

## Scope and checks

Tests cover Earth-, Mars- and Jupiter-like example columns, gas-mixture
validation, ideal-gas/hydrostatic scaling, reference levels, aerosol extent
and extended-column warnings. Generation tests cover isolated-star flux,
eccentric-orbit averages, stellar and circumbinary hosts, nested companions,
initial-phase independence, activity/retention trends, cold condensation,
wire validation and deterministic population diversity. Inspector tests cover
unknown, airless, surface and envelope records.

Not modelled: physical escape rates, atmospheric evolution, chemical equilibrium,
resolved phase diagrams/hydrospheres, greenhouse heating, nonideal dense gases,
dissociation/ionisation, upper-air
temperature structure, winds or variable-gravity envelopes. Physically
valid input ranges are not evidence that these approximations hold in every
regime. In particular, dense/hot atmospheres and deep giant envelopes will
need better treatment. The eventual optical conversion must remain separate:
visible absorption is not greenhouse absorption, and CO₂ does not by itself
make a sky orange.
