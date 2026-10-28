//! Transport record for an experimental layered or fallback solve. Never written into
//! `Planet::physical`, whose validation re-derives the legacy structure.
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
    let surface_pressure_pa = if s.envelope().is_some() {
        1e5
    } else {
        air.map_or(0.0, |a| a.pressure_pa())
    };
    let mut layers = Vec::new();
    if let Some(envelope) = s.envelope() {
        for (index, r) in envelope.regions().iter().enumerate().rev() {
            let mut layer = region(
                r.inventory(),
                r.inner(),
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
    let surface = solved.surface();
    LayeredPlanet {
        model: solved.model_id().into(),
        fallback_reason: None,
        // All current planet closures include cold proxies/prescribed thermal
        // states. Checked local equations never certify the whole structure.
        model_fidelity: layers
            .iter()
            .fold(ModelFidelity::Approximate, |a, l| a.max(l.model_fidelity)),
        model_limitations: strings(s.approximation_notes()),
        physical: PlanetPhysical {
            materials: Material::ALL
                .into_iter()
                .map(|m| {
                    let suspended = s.envelope().map_or(0.0, |e| {
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
            radius_reference: if s.envelope().is_some() {
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
        blowoff_kg: solved.blowoff.total_mass_kg(),
        layers,
    }
}

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
    let layers = f
        .regions
        .iter()
        .rev()
        .map(|r| {
            let envelope = r.kind == fallback::Kind::Envelope;
            Layer {
                phase_allocations: Vec::new(),
                model_fidelity: fidelity,
                model_limitations: vec![
                    if envelope {
                        "H/He envelope mass-radius fit; prescribed thinning density profile"
                    } else {
                        "Seager-fit compressed density; phase and temperature unresolved"
                    }
                    .into(),
                ],
                kind: r.kind.id().into(),
                material: dominant(&r.inventory).into(),
                inner_radius_m: r.inner_radius_m,
                outer_radius_m: r.outer_radius_m,
                mass_kg: r.inventory.total_mass_kg(),
                inner_pressure_pa: r.inner_pressure_pa,
                outer_pressure_pa: (!envelope).then_some(r.outer_pressure_pa),
                temperature_k: envelope.then_some(f.surface_temperature_k),
                model: fallback::MODEL_ID.into(),
            }
        })
        .collect();
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
        fallback_reason: Some(f.reason.to_string()),
        model_fidelity: fidelity,
        model_limitations: strings(&f.notes),
        physical: PlanetPhysical {
            materials: Material::ALL
                .into_iter()
                .map(|m| MaterialBudget {
                    material: m.id().into(),
                    source_kg: f.source.mass_kg(m),
                    retained_kg: f.retained.mass_kg(m),
                    escaped_kg: f.escaped.mass_kg(m),
                    interior_kg: in_kind(m, &[K::Core, K::Mantle]),
                    condensed_kg: in_kind(m, &[K::Water, K::Volatiles]),
                    gas_kg: in_kind(m, &[K::Envelope]),
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
            approximations: vec![fallback::MODEL_ID.into()],
        },
        surface: f.surface().into(),
        surface_temperature_k: f.surface_temperature_k,
        surface_pressure_pa: f.surface_pressure_pa,
        greenhouse_optical_depth: None,
        one_bar_radius_m: f.one_bar_radius_m,
        blowoff_kg: 0.0,
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
