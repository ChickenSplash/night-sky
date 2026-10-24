# Planet composition refactor

Starter proposal, not implemented. Replace class-first generation with physical
inventories and properties, then derive descriptive labels. Builds on the
[atmosphere work](atmospheres.md#checkpoint-5--every-planet-with-air).

1. **Physical inventories.** Represent iron, silicates, water/ices and H₂/He
   with explicit mass budgets. Keep bulk inventories distinct from atmospheric
   gas fractions and condensed reservoirs.
2. **Generation before labels.** Use formation conditions and approximate
   retention to draw inventories, not a class that dictates composition.
   Keep uncertain prescriptions isolated and deterministic.
3. **Consistent properties.** Derive approximate radius, density and atmospheric
   supply from the inventories, mass and thermal state. Replace class-based
   envelope probabilities and radius fits; conserve mass between reservoirs.
4. **Classification afterwards.** Derive rocky, water-rich and envelope-dominated
   descriptions from physical properties. Keep dynamical categories, such as
   dwarf planet, separate. Audit remaining class-dependent spin, shape and
   appearance rules rather than treating labels as physical constraints.
5. **Preserve subsystem boundaries.** Reuse atmospheric validation, optical
   conversion and rendering. Distinguish solid surfaces from envelope reference
   levels; a chosen radius must not become an artificial future descent limit.
6. **Validate in stages.** Check reference worlds, mass budgets, radius/density
   trends and population distributions. Review visual changes before committing
   implementation checkpoints.

Start with tunable approximations, not a full formation, interior, atmospheric
evolution or climate simulation. Existing generated populations may change.
