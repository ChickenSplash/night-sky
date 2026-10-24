# Physical atmospheres

`crates/simulation/src/system/atmosphere.rs` provides validated physical
states and a first column approximation. It is independent of Bevy, shaders,
planet generation and wire models. **Generated planets do not use it yet:**
the client still uses its hand-set atmosphere on the selected planet.
See [the plan](../plans/atmospheres.md#checkpoint-5--every-planet-with-air)
for generation, optical conversion and integration work.

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
surface climate. Those estimates will be separate later. All stored units
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

## Scope and checks

Tests cover Earth-, Mars- and Jupiter-like example columns, gas-mixture
validation, ideal-gas/hydrostatic scaling, reference levels, aerosol extent
and extended-column warnings. Fixtures use prescribed temperatures and
compositions, not generated climate or visual presets.

Not modelled: retention, atmospheric evolution, chemistry, condensation,
greenhouse heating, nonideal dense gases, dissociation/ionisation, upper-air
temperature structure, winds or variable-gravity envelopes. Physically
valid input ranges are not evidence that these approximations hold in every
regime. In particular, dense/hot atmospheres and deep giant envelopes will
need better treatment. The eventual optical conversion must remain separate:
visible absorption is not greenhouse absorption, and CO₂ does not by itself
make a sky orange.
