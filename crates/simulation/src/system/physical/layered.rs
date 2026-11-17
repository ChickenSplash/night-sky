//! Transport record for a layered or fallback solve. A generated planet's
//! `Planet::physical` is a verbatim copy of its result's record.
use crate::{
    system::{
        composition::{Inventory, Material},
        materials::water::Phase,
        structure::{
            coupled::CondensedStructure,
            dry,
            fallback::{self, Fallback},
            prototype::{Outcome, Solved},
            water::{self, PhaseKnowledge},
        },
    },
    units::GRAVITATIONAL_CONSTANT,
};
use space_shared::{
    Layer, LayeredPlanet, MaterialBudget, ModelFidelity, PlanetPhysical, RadiusReference,
};
use std::f64::consts::PI;

pub fn layered(solved: &Solved) -> LayeredPlanet {
    let s = &solved.structure;
    let air = s.atmosphere();
    let surface_pressure_pa = if let Some(envelope) = s.envelope() {
        envelope.reference_pressure_pa()
    } else {
        air.map_or(0.0, |a| a.pressure_pa())
    };
    let mut layers = Vec::new();
    if let Some(envelope) = s.envelope() {
        for (index, r) in envelope.regions().iter().enumerate().rev() {
            // The gas and the condensed body are solved towards each other and
            // meet within the solver's tolerance; the record has one boundary.
            let inner = if index == 0 {
                dry::Sample {
                    radius_m: s.condensed().radius_m(),
                    ..r.inner()
                }
            } else {
                r.inner()
            };
            let mut layer = region(
                r.inventory(),
                inner,
                r.outer(),
                envelope.thermal().temperature_k(r.outer().pressure_pa).ok(),
                "envelope",
                envelope.region_density_model_id(r),
            );
            if layer.model == crate::system::materials::hhe_mixture::EFFECTIVE_MODEL_ID {
                layer.model_limitations.extend(strings(
                    crate::system::materials::effective_secondary::APPROXIMATION_NOTES,
                ));
            }
            if envelope.uses_extended_mixture() {
                layer.model_limitations.push("Heavy enrichment uses additive volumes and the unchanged H/He-oriented thermal prescription; composition-dependent opacity and heat capacity unresolved".into());
            }
            if envelope.uses_suspended_condensates() {
                layer
                    .model_limitations
                    .extend(strings(crate::system::materials::suspended::NOTES));
                let condensate = envelope.region_condensate(index);
                layer.phase_allocations = Material::ALL
                    .into_iter()
                    .filter(|&m| r.inventory().mass_kg(m) > 0.0)
                    .map(|m| space_shared::PhaseAllocation {
                        material: m.id().into(),
                        vapour_kg: r.inventory().mass_kg(m) - condensate.mass_kg(m),
                        condensate_kg: condensate.mass_kg(m),
                    })
                    .collect();
            }
            layers.push(layer);
        }
    }
    if let Some(air) = air {
        let top = s
            .column()
            .ok()
            .flatten()
            .map_or(s.surface_radius_m(), |c| s.surface_radius_m() + c.extent_m);
        layers.push(Layer {
            materials: materials(s.gas_inventory()),
            phase_allocations: Vec::new(),
            model_fidelity: ModelFidelity::Approximate,
            model_limitations: strings(s.approximation_notes()),
            kind: "atmosphere".into(),
            material: dominant(s.gas_inventory()).into(),
            inner_radius_m: s.surface_radius_m(),
            outer_radius_m: top,
            mass_kg: s.gas_inventory().total_mass_kg(),
            inner_pressure_pa: air.pressure_pa(),
            outer_pressure_pa: None,
            temperature_k: Some(s.surface_temperature_k()),
            model: s.column_model_id().into(),
        });
    }
    if s.deposit_thickness_m() > 0.0 {
        layers.push(Layer {
            materials: materials(s.deposit_inventory()),
            phase_allocations: Vec::new(),
            model_fidelity: ModelFidelity::Approximate,
            model_limitations: vec![
                "Prescribed deposit density; phase and internal thermal state unresolved".into(),
            ],
            kind: "frost".into(),
            material: dominant(s.deposit_inventory()).into(),
            inner_radius_m: s.condensed().radius_m(),
            outer_radius_m: s.surface_radius_m(),
            mass_kg: s.deposit_inventory().total_mass_kg(),
            inner_pressure_pa: s.condensed_load_pa(),
            outer_pressure_pa: Some(surface_pressure_pa),
            temperature_k: Some(s.surface_temperature_k()),
            model: s.model_id().into(),
        });
    }
    let (condensed_model, mut condensed): (&str, Vec<Layer>) = match s.condensed() {
        CondensedStructure::Dry(d) => (
            d.model_id(),
            d.regions()
                .iter()
                .map(|r| {
                    region(
                        r.inventory(),
                        r.inner(),
                        r.outer(),
                        None,
                        refractory(r.inventory()),
                        d.model_id(),
                    )
                })
                .collect(),
        ),
        CondensedStructure::Water(w) => (
            w.model_id(),
            w.regions()
                .iter()
                .map(|r| {
                    let mut layer = region(
                        r.inventory(),
                        r.inner(),
                        r.outer(),
                        r.temperature_k(),
                        water_kind(r),
                        r.density_model_id(),
                    );
                    layer
                        .model_limitations
                        .extend(strings(r.approximation_notes()));
                    if r.density_model_id()
                        == crate::system::materials::water::cold_ice::IH_MODEL_ID
                    {
                        layer.model_fidelity = ModelFidelity::Extrapolated;
                    }
                    layer
                })
                .collect(),
        ),
    };
    condensed.reverse();
    layers.extend(condensed);

    let condensed_inventory = s.condensed().retained_inventory();
    let retained = s.retained_inventory();
    let mass = s.mass_kg();
    let radius = s.surface_radius_m();
    let mut models: Vec<String> = Vec::new();
    let envelope_models: &[&str] = if s.envelope().is_some_and(|e| e.uses_effective_components()) {
        &[
            crate::system::structure::envelope::THERMAL_MODEL_ID,
            crate::system::materials::effective_secondary::MODEL_ID,
            crate::system::materials::hhe::MODEL_ID,
            crate::system::materials::hhe::COLD_MODEL_ID,
            "bounded_pure_he_melting_screen_extension_v1",
        ]
    } else if s.envelope().is_some_and(|e| e.uses_molecular_mixture()) {
        &[
            crate::system::structure::envelope::THERMAL_MODEL_ID,
            crate::system::materials::molecular::MODEL_ID,
            crate::system::materials::hhe::MODEL_ID,
            crate::system::materials::hhe::COLD_MODEL_ID,
            "bounded_pure_he_melting_screen_extension_v1",
        ]
    } else if s.envelope().is_some() {
        &[
            crate::system::structure::envelope::THERMAL_MODEL_ID,
            crate::system::structure::envelope::TRACE_MODEL_ID,
            crate::system::materials::hhe::COLD_MODEL_ID,
            "bounded_pure_he_melting_screen_extension_v1",
        ]
    } else {
        &[]
    };
    for model in [solved.model_id(), condensed_model, s.column_model_id()]
        .into_iter()
        .chain(envelope_models.iter().copied())
        .chain(layers.iter().map(|l| l.model.as_str()))
    {
        if !models.iter().any(|m| m == model) {
            models.push(model.into());
        }
    }
    if s.envelope().is_some_and(|e| e.uses_molecular_mixture())
        && !s.envelope().is_some_and(|e| e.uses_suspended_condensates())
        && layers.iter().any(|l| {
            l.kind == "envelope"
                && l.model != crate::system::materials::hhe_mixture::EFFECTIVE_MODEL_ID
        })
        && !models
            .iter()
            .any(|m| m == crate::system::materials::molecular::MODEL_ID)
    {
        models.push(crate::system::materials::molecular::MODEL_ID.into());
    }
    if s.envelope().is_some_and(|e| e.uses_suspended_condensates()) {
        models.retain(|m| m != crate::system::materials::molecular::MODEL_ID);
        if !models
            .iter()
            .any(|m| m == crate::system::materials::suspended::MODEL_ID)
        {
            models.push(crate::system::materials::suspended::MODEL_ID.into());
        }
    }
    if s.envelope().is_some_and(|e| e.has_prescribed_allocation())
        && s.gas_inventory().mass_kg(Material::Water) > 0.0
    {
        models.push(crate::system::structure::envelope::WATER_TRACE_MODEL_ID.into());
    }
    let surface = solved.surface();
    let mut limitations = strings(s.approximation_notes());
    if s.envelope().is_some_and(|e| e.has_prescribed_allocation()) {
        limitations.push("Generation fixes the atmospheric reservoir and thermal reference gravity; local deep phase allocations do not repartition the visible column".into());
        if s.gas_inventory().mass_kg(Material::Water) > 0.0 {
            limitations.push("Prescribed water vapour up to 0.301% of envelope mass shares the other carrier's specific volume; not a water-mixture EOS or deep water phase prediction".into());
        }
    }
    if s.envelope().is_some_and(|e| e.one_bar_radius_m().is_none()) {
        limitations.push("Gas base is below one bar: radius, pressure, temperature and gravity reference the condensed boundary; water, when present, remains phase-unresolved".into());
    }
    LayeredPlanet {
        model: solved.model_id().into(),
        fallback_reason: None,
        // All current planet closures include cold proxies/prescribed thermal
        // states. Checked local equations never certify the whole structure.
        model_fidelity: layers
            .iter()
            .fold(ModelFidelity::Approximate, |a, l| a.max(l.model_fidelity)),
        model_limitations: limitations,
        physical: PlanetPhysical {
            materials: Material::ALL
                .into_iter()
                .map(|m| {
                    let suspended = s.envelope().map_or(0.0, |e| {
                        if e.has_prescribed_allocation() {
                            return 0.0;
                        }
                        (0..e.regions().len())
                            .map(|i| e.region_condensate(i).mass_kg(m))
                            .sum::<f64>()
                    });
                    let water = if m == Material::Water {
                        condensed_inventory.mass_kg(m)
                    } else {
                        0.0
                    };
                    MaterialBudget {
                        material: m.id().into(),
                        source_kg: solved.source.mass_kg(m),
                        retained_kg: retained.mass_kg(m),
                        escaped_kg: solved.escaped.mass_kg(m),
                        interior_kg: condensed_inventory.mass_kg(m) - water,
                        condensed_kg: water + s.deposit_inventory().mass_kg(m) + suspended,
                        gas_kg: s.gas_inventory().mass_kg(m) - suspended,
                    }
                })
                .collect(),
            retained_mass_kg: mass,
            reference_radius_m: radius,
            condensed_radius_m: s.condensed().radius_m(),
            mean_density_kg_m3: mass / (4.0 / 3.0 * PI * radius.powi(3)),
            reference_gravity_m_s2: s.envelope().map_or_else(
                || GRAVITATIONAL_CONSTANT * mass / radius.powi(2),
                |e| e.reference_gravity_m_s2(),
            ),
            h_he_mass_fraction: (retained.mass_kg(Material::Hydrogen)
                + retained.mass_kg(Material::Helium))
                / mass,
            inertia_factor: s.inertia_kg_m2() / (mass * radius.powi(2)),
            // Only bare rock and bare ice-Ih count as ground. Liquid, frost of
            // unmodelled phase and unresolved water never grant a solid surface.
            radius_reference: if s.envelope().is_some_and(|e| e.one_bar_radius_m().is_some()) {
                RadiusReference::OneBar
            } else if matches!(surface, "rock" | "ice") {
                RadiusReference::SolidSurface
            } else {
                RadiusReference::UnresolvedVolatileBoundary
            },
            approximations: models,
        },
        surface: surface.into(),
        surface_temperature_k: s.surface_temperature_k(),
        surface_pressure_pa,
        greenhouse_optical_depth: s.greenhouse_optical_depth(),
        one_bar_radius_m: s.one_bar_radius_m().ok().flatten(),
        // Generation decides escape; a standalone solve reports none.
        blowoff_kg: 0.0,
        layers,
    }
}

pub(crate) const PRESCRIBED_BOUNDARY_MODEL_ID: &str =
    "generated_prescribed_atmospheric_boundary_v1";

pub fn outcome(outcome: &Outcome) -> LayeredPlanet {
    match outcome {
        Outcome::Layered(solved) => layered(solved),
        Outcome::Fallback(f) => fallback(f),
    }
}

pub fn fallback(f: &Fallback) -> LayeredPlanet {
    let fidelity = if f.extrapolated {
        ModelFidelity::Extrapolated
    } else {
        ModelFidelity::Approximate
    };
    let mass = f.mass_kg();
    let radius = f.radius_m;
    let one_bar = f.reference == RadiusReference::OneBar;
    let layers: Vec<Layer> = f
        .regions
        .iter()
        .rev()
        .map(|r| {
            let envelope = r.kind == fallback::Kind::Envelope;
            let atmosphere = r.kind == fallback::Kind::Atmosphere;
            let mut limitations = vec![
                match r.kind {
                    fallback::Kind::Atmosphere if one_bar => {
                        "Isothermal column above the one-bar level, holding the gas one bar supports there"
                    }
                    fallback::Kind::Atmosphere => {
                        "Finite mass-normalised isothermal gas column; weak-binding pressure remains approximate"
                    }
                    fallback::Kind::Frost => {
                        "Even global frost layer of fixed solid density; phase unresolved"
                    }
                    fallback::Kind::Envelope if r.tabulated_pa.is_some() => {
                        "H/He envelope from the tabulated base to the tabulated one-bar radius; prescribed thermal profile, no cooling evolution"
                    }
                    fallback::Kind::Envelope => {
                        "Gas to the one-bar level of its own column; prescribed thinning density profile"
                    }
                    fallback::Kind::Core | fallback::Kind::Mantle => {
                        "Boundary and pressures interpolated from the layered solver's table; cold compression proxy, temperature, mineralogy and phase unresolved"
                    }
                    fallback::Kind::Water => {
                        "Water-rich layer inside the tabulated boundaries; phase unresolved"
                    }
                    fallback::Kind::Volatiles => {
                        "Condensed volatiles as a water-density proxy inside the tabulated radius; phase unresolved"
                    }
                    _ => {
                        "Phase boundary from the layered solver's water models on the decided isotherm; one prescribed temperature, not a thermal evolution solution"
                    }
                }
                .to_owned(),
            ];
            let condensed = r.condensate.total_mass_kg() > 0.0;
            if condensed {
                limitations.push(fallback::decide::SUSPENDED_NOTE.into());
            }
            Layer {
                materials: materials(&r.inventory),
                phase_allocations: if condensed {
                    Material::ALL
                        .into_iter()
                        .filter(|&m| r.inventory.mass_kg(m) > 0.0)
                        .map(|m| space_shared::PhaseAllocation {
                            material: m.id().into(),
                            vapour_kg: r.inventory.mass_kg(m) - r.condensate.mass_kg(m),
                            condensate_kg: r.condensate.mass_kg(m),
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                model_fidelity: fidelity,
                model_limitations: limitations,
                kind: r.kind.id().into(),
                material: dominant(&r.inventory).into(),
                inner_radius_m: r.inner_radius_m,
                outer_radius_m: r.outer_radius_m,
                mass_kg: r.inventory.total_mass_kg(),
                inner_pressure_pa: r.inner_pressure_pa,
                // An envelope that is the outermost layer has no stated top.
                outer_pressure_pa: (!envelope || r.outer_pressure_pa > 0.0)
                    .then_some(r.outer_pressure_pa),
                temperature_k: (envelope
                    || atmosphere
                    || r.kind == fallback::Kind::Frost
                    || (r.kind.is_water() && r.kind != fallback::Kind::Water))
                    .then_some(f.surface_temperature_k),
                model: fallback::MODEL_ID.into(),
            }
        })
        .collect();
    let mut limitations = strings(&f.notes);
    if f.quality.filled > 0 {
        limitations.push(format!(
            "Structure table: {} of the {} knots read hold a fill where the layered solver refuses ({})",
            f.quality.filled,
            f.quality.corners,
            f.quality
                .reasons()
                .map(|r| r.label())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if f.quality.outside {
        limitations.push(
            "Structure table: outside its tabulated range, on an edge or a continuation law".into(),
        );
    }
    let mut approximations = vec![fallback::MODEL_ID.to_owned()];
    // The deep phase detail does not move the boundary column generation
    // prescribes, which optics and drag read.
    if layers.iter().any(|l| !l.phase_allocations.is_empty()) {
        approximations.push(PRESCRIBED_BOUNDARY_MODEL_ID.into());
    }
    let in_kind = |m: Material, kinds: &[fallback::Kind]| -> f64 {
        f.regions
            .iter()
            .filter(|r| kinds.contains(&r.kind))
            .map(|r| r.inventory.mass_kg(m))
            .sum()
    };
    use fallback::Kind as K;
    LayeredPlanet {
        model: fallback::MODEL_ID.into(),
        fallback_reason: f.reason.as_ref().map(ToString::to_string),
        model_fidelity: fidelity,
        model_limitations: limitations,
        physical: PlanetPhysical {
            materials: Material::ALL
                .into_iter()
                .map(|m| MaterialBudget {
                    material: m.id().into(),
                    source_kg: f.source.mass_kg(m),
                    retained_kg: f.retained.mass_kg(m),
                    escaped_kg: f.escaped.mass_kg(m),
                    interior_kg: in_kind(m, &[K::Core, K::Mantle]),
                    condensed_kg: in_kind(
                        m,
                        &[
                            K::Water,
                            K::Ocean,
                            K::Ice,
                            K::DeepIce,
                            K::HotWater,
                            K::Coexistence,
                            K::Volatiles,
                            K::Frost,
                        ],
                    ),
                    gas_kg: in_kind(m, &[K::Envelope, K::Atmosphere]),
                })
                .collect(),
            retained_mass_kg: mass,
            reference_radius_m: radius,
            condensed_radius_m: f.condensed_radius_m,
            mean_density_kg_m3: mass / (4.0 / 3.0 * PI * radius.powi(3)),
            reference_gravity_m_s2: GRAVITATIONAL_CONSTANT * mass / radius.powi(2),
            h_he_mass_fraction: (f.retained.mass_kg(Material::Hydrogen)
                + f.retained.mass_kg(Material::Helium))
                / mass,
            inertia_factor: f.inertia_kg_m2 / (mass * radius.powi(2)),
            radius_reference: f.reference,
            approximations,
        },
        surface: f.surface().into(),
        surface_temperature_k: f.surface_temperature_k,
        surface_pressure_pa: f.surface_pressure_pa,
        greenhouse_optical_depth: f.optical_depth,
        one_bar_radius_m: f.one_bar_radius_m,
        blowoff_kg: f.blowoff.total_mass_kg(),
        layers,
    }
}

fn region(
    inventory: &Inventory,
    inner: dry::Sample,
    outer: dry::Sample,
    temperature_k: Option<f64>,
    kind: &str,
    model: &str,
) -> Layer {
    Layer {
        materials: materials(inventory),
        phase_allocations: Vec::new(),
        model_fidelity: ModelFidelity::Approximate,
        model_limitations: vec![match kind {
            "envelope" => "Prescribed thermal profile and uniform composition; no cooling evolution or full mixture phase equilibrium",
            "core" | "mantle" => "Cold compression proxy; temperature, mineralogy and phase unresolved",
            _ => "Prescribed water temperature/structure; not a thermal evolution solution",
        }.into()],
        kind: kind.into(),
        material: dominant(inventory).into(),
        inner_radius_m: inner.radius_m,
        outer_radius_m: outer.radius_m,
        mass_kg: inventory.total_mass_kg(),
        inner_pressure_pa: inner.pressure_pa,
        outer_pressure_pa: Some(outer.pressure_pa),
        temperature_k,
        model: model.into(),
    }
}

fn materials(inventory: &Inventory) -> Vec<space_shared::LayerMaterial> {
    Material::ALL
        .into_iter()
        .filter_map(|m| {
            let mass_kg = inventory.mass_kg(m);
            (mass_kg > 0.0).then(|| space_shared::LayerMaterial {
                material: m.id().into(),
                mass_kg,
            })
        })
        .collect()
}

fn strings(notes: &[&str]) -> Vec<String> {
    notes.iter().map(|s| (*s).into()).collect()
}

fn dominant(inventory: &Inventory) -> &'static str {
    Material::ALL
        .into_iter()
        .max_by(|a, b| inventory.mass_kg(*a).total_cmp(&inventory.mass_kg(*b)))
        .unwrap()
        .id()
}

fn refractory(inventory: &Inventory) -> &'static str {
    if inventory.mass_kg(Material::Metal) > inventory.mass_kg(Material::Silicate) {
        "core"
    } else {
        "mantle"
    }
}

fn water_kind(region: &water::Region) -> &'static str {
    if region.density_model_id() == crate::system::structure::envelope::WATER_MODEL_ID {
        return "water_compression";
    }
    match region.phase() {
        PhaseKnowledge::Unresolved => refractory(region.inventory()),
        PhaseKnowledge::Water(Phase::Liquid) => "ocean",
        PhaseKnowledge::Water(Phase::IceIh) => "ice",
        PhaseKnowledge::Water(_) => "water_coexistence",
        PhaseKnowledge::EffectiveHighPressureIce => "deep_ice",
        PhaseKnowledge::UnresolvedHotWater => "hot_water",
        PhaseKnowledge::UnresolvedCompression => "water_compression",
    }
}

#[cfg(test)]
mod tests;
