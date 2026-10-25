//! Every searchable kind of object and property, declared once. The query
//! parser, engine, CLI and browser all read these tables; none of them names
//! a kind or property in code of its own.
use serde::Serialize;

/// A browsing group, not a scientific taxonomy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Family {
    pub id: &'static str,
    pub label: &'static str,
}

pub const FAMILIES: &[Family] = &[
    Family {
        id: "stars",
        label: "Stars & remnants",
    },
    Family {
        id: "planetary",
        label: "Planetary bodies",
    },
    Family {
        id: "moons",
        label: "Moons & satellites",
    },
    Family {
        id: "small_bodies",
        label: "Small bodies",
    },
    Family {
        id: "systems",
        label: "Systems",
    },
    Family {
        id: "deep_sky",
        label: "Clusters, nebulae & galaxies",
    },
    Family {
        id: "craft",
        label: "Craft",
    },
];

/// Where a kind's candidates come from in a scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Cell stars and the systems generated from them.
    Cells,
    /// Live objects outside the generated galaxy, such as craft, which the
    /// client merges into results (`live`).
    Live,
    /// Not generated yet: registered, but hidden and refused.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Kind {
    pub id: &'static str,
    pub label: &'static str,
    pub family: &'static str,
    /// What it belongs to or orbits, if anything.
    pub parent: Option<&'static str>,
    pub source: Source,
}

impl Kind {
    pub fn available(&self) -> bool {
        self.source != Source::None
    }
}

const fn kind(
    id: &'static str,
    label: &'static str,
    family: &'static str,
    parent: Option<&'static str>,
    source: Source,
) -> Kind {
    Kind {
        id,
        label,
        family,
        parent,
        source,
    }
}

pub const KINDS: &[Kind] = &[
    kind("star", "Star", "stars", Some("system"), Source::Cells),
    kind("system", "Star system", "systems", None, Source::Cells),
    kind("planet", "Planet", "planetary", Some("star"), Source::Cells),
    kind(
        "brown_dwarf",
        "Brown dwarf",
        "stars",
        Some("system"),
        Source::None,
    ),
    kind(
        "neutron_star",
        "Neutron star",
        "stars",
        Some("system"),
        Source::None,
    ),
    kind(
        "black_hole",
        "Black hole",
        "stars",
        Some("system"),
        Source::None,
    ),
    kind(
        "rogue_planet",
        "Rogue planet",
        "planetary",
        None,
        Source::None,
    ),
    kind("moon", "Moon", "moons", Some("planet"), Source::None),
    kind(
        "asteroid",
        "Asteroid",
        "small_bodies",
        Some("star"),
        Source::None,
    ),
    kind("comet", "Comet", "small_bodies", Some("star"), Source::None),
    kind("cluster", "Star cluster", "deep_sky", None, Source::None),
    kind("nebula", "Nebula", "deep_sky", None, Source::None),
    kind("galaxy", "Galaxy", "deep_sky", None, Source::None),
    kind("ship", "Ship", "craft", Some("planet"), Source::Live),
    kind("station", "Station", "craft", Some("planet"), Source::None),
    kind("rover", "Rover", "craft", Some("planet"), Source::None),
    kind("probe", "Probe", "craft", Some("star"), Source::None),
];

/// What a property's filter holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Values {
    /// A range of numbers in the property's unit; open ends allowed.
    Range { integer: bool },
    /// One or more of these, by ID and label.
    Choice {
        options: &'static [(&'static str, &'static str)],
    },
    /// Yes or no.
    Flag,
}

/// Which generated value a property reads. Only the engine interprets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Attr {
    StarClass,
    TemperatureK,
    MassSolar,
    RadiusSolar,
    LuminositySolar,
    Multiple,
    StarCount,
    PlanetCount,
    PlanetClass,
    RadiusKm,
    MassEarth,
    SemiMajorAu,
    Eccentricity,
    PeriodDays,
    Resonant,
    TidallyLocked,
    AltitudeKm,
    SpeedKms,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Property {
    pub id: &'static str,
    pub label: &'static str,
    /// The one unit values are given in, if any.
    pub unit: Option<&'static str>,
    pub values: Values,
    /// Kinds it applies to.
    pub kinds: &'static [&'static str],
    #[serde(skip)]
    pub(crate) attr: Attr,
}

pub const STAR_CLASSES: &[(&str, &str)] = &[
    ("o", "O · blue"),
    ("b", "B · blue-white"),
    ("a", "A · white"),
    ("f", "F · yellow-white"),
    ("g", "G · yellow dwarf"),
    ("k", "K · orange dwarf"),
    ("m", "M · red dwarf"),
    ("giant", "Giant"),
    ("supergiant", "Supergiant"),
    ("white_dwarf", "White dwarf"),
];

pub const PLANET_CLASSES: &[(&str, &str)] = &[
    ("metal_rich", "Metal-rich planet"),
    ("water_rich", "Water-rich planet"),
    ("rocky", "Rocky planet"),
    ("sub_neptune", "Sub-Neptune"),
    ("envelope_world", "Envelope world"),
    ("gas_giant", "Gas giant"),
];

const STAR: &[&str] = &["star"];
const PLANET: &[&str] = &["planet"];
const SYSTEM: &[&str] = &["system"];
const CRAFT: &[&str] = &["ship", "station", "rover", "probe"];
const NUMBER: Values = Values::Range { integer: false };
const COUNT: Values = Values::Range { integer: true };

const fn property(
    id: &'static str,
    label: &'static str,
    unit: Option<&'static str>,
    values: Values,
    kinds: &'static [&'static str],
    attr: Attr,
) -> Property {
    Property {
        id,
        label,
        unit,
        values,
        kinds,
        attr,
    }
}

pub const PROPERTIES: &[Property] = &[
    property(
        "star_class",
        "Star class",
        None,
        Values::Choice {
            options: STAR_CLASSES,
        },
        STAR,
        Attr::StarClass,
    ),
    property(
        "temperature_k",
        "Surface temperature",
        Some("K"),
        NUMBER,
        STAR,
        Attr::TemperatureK,
    ),
    property(
        "mass_solar",
        "Star mass",
        Some("M☉"),
        NUMBER,
        STAR,
        Attr::MassSolar,
    ),
    property(
        "radius_solar",
        "Star radius",
        Some("R☉"),
        NUMBER,
        STAR,
        Attr::RadiusSolar,
    ),
    property(
        "luminosity_solar",
        "Luminosity",
        Some("L☉"),
        NUMBER,
        STAR,
        Attr::LuminositySolar,
    ),
    property(
        "multiple",
        "In a multiple system",
        None,
        Values::Flag,
        &["star", "system"],
        Attr::Multiple,
    ),
    property(
        "star_count",
        "Stars in system",
        None,
        COUNT,
        SYSTEM,
        Attr::StarCount,
    ),
    property(
        "planet_count",
        "Planets in system",
        None,
        COUNT,
        SYSTEM,
        Attr::PlanetCount,
    ),
    property(
        "planet_class",
        "Planet class",
        None,
        Values::Choice {
            options: PLANET_CLASSES,
        },
        PLANET,
        Attr::PlanetClass,
    ),
    property(
        "radius_km",
        "Planet radius",
        Some("km"),
        NUMBER,
        PLANET,
        Attr::RadiusKm,
    ),
    property(
        "mass_earth",
        "Planet mass",
        Some("M⊕"),
        NUMBER,
        PLANET,
        Attr::MassEarth,
    ),
    property(
        "semi_major_au",
        "Orbit size",
        Some("AU"),
        NUMBER,
        PLANET,
        Attr::SemiMajorAu,
    ),
    property(
        "eccentricity",
        "Eccentricity",
        None,
        NUMBER,
        PLANET,
        Attr::Eccentricity,
    ),
    property(
        "period_days",
        "Orbital period",
        Some("days"),
        NUMBER,
        PLANET,
        Attr::PeriodDays,
    ),
    property(
        "resonant",
        "In resonance with the planet inside it",
        None,
        Values::Flag,
        PLANET,
        Attr::Resonant,
    ),
    property(
        "tidally_locked",
        "Tidally locked",
        None,
        Values::Flag,
        PLANET,
        Attr::TidallyLocked,
    ),
    property(
        "altitude_km",
        "Altitude above the body it is around",
        Some("km"),
        NUMBER,
        CRAFT,
        Attr::AltitudeKm,
    ),
    property(
        "speed_kms",
        "Speed relative to the body it is around",
        Some("km/s"),
        NUMBER,
        CRAFT,
        Attr::SpeedKms,
    ),
];

/// Which link between objects a relationship follows. Only the engine
/// interprets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Link {
    /// A planet's direct host: its star, or every star of the pair it orbits.
    /// A craft's orbit reference: a planet, a star, or every star of a pair.
    Orbits,
    /// Anything else in the same system, or the system itself.
    System,
}

/// A relationship: `orbits.star_class=m` holds when what the object orbits
/// has `star_class=m`. All of one relationship's filters must hold for the
/// same related object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Relation {
    /// Its prefix in the text syntax, before the dot.
    pub id: &'static str,
    pub label: &'static str,
    /// Kinds that can have it.
    pub subjects: &'static [&'static str],
    /// Kinds it can lead to.
    pub targets: &'static [&'static str],
    #[serde(skip)]
    pub(crate) link: Link,
}

pub const RELATIONS: &[Relation] = &[
    Relation {
        id: "orbits",
        label: "Orbits",
        subjects: &["planet", "ship", "station", "rover", "probe"],
        targets: &["star", "planet"],
        link: Link::Orbits,
    },
    Relation {
        id: "system",
        label: "In a system with",
        subjects: &["star", "system", "planet"],
        targets: &["star", "system", "planet"],
        link: Link::System,
    },
];

pub fn relation_index(id: &str) -> Option<usize> {
    RELATIONS.iter().position(|r| r.id == id)
}

pub fn kind_index(id: &str) -> Option<usize> {
    KINDS.iter().position(|k| k.id == id)
}

pub fn property_index(id: &str) -> Option<usize> {
    PROPERTIES.iter().position(|p| p.id == id)
}

pub fn family(id: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.id == id)
}

/// Kinds, as a bit per `KINDS` index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KindSet(pub u32);

impl KindSet {
    pub fn of(index: usize) -> Self {
        Self(1 << index)
    }

    pub fn available() -> Self {
        Self::matching(|k| k.available())
    }

    pub fn matching(f: impl Fn(&Kind) -> bool) -> Self {
        Self(
            KINDS
                .iter()
                .enumerate()
                .filter(|(_, k)| f(k))
                .fold(0, |bits, (i, _)| bits | 1 << i),
        )
    }

    pub fn has(self, id: &str) -> bool {
        kind_index(id).is_some_and(|i| self.0 & 1 << i != 0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn and(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub fn ids(self) -> impl Iterator<Item = &'static str> {
        KINDS
            .iter()
            .enumerate()
            .filter(move |(i, _)| self.0 & 1 << i != 0)
            .map(|(_, k)| k.id)
    }
}

impl Property {
    pub fn applies(&self) -> KindSet {
        KindSet::matching(|k| self.kinds.contains(&k.id))
    }
}

impl Relation {
    pub fn subject_kinds(&self) -> KindSet {
        KindSet::matching(|k| self.subjects.contains(&k.id) && k.available())
    }

    pub fn target_kinds(&self) -> KindSet {
        KindSet::matching(|k| self.targets.contains(&k.id) && k.available())
    }
}

/// The whole registry, for discovery (`GET /catalogue`). Unavailable kinds
/// are left out.
#[derive(Serialize)]
pub struct Catalogue {
    pub families: &'static [Family],
    pub kinds: Vec<Kind>,
    pub properties: &'static [Property],
    pub relations: &'static [Relation],
}

pub fn catalogue() -> Catalogue {
    Catalogue {
        families: FAMILIES,
        kinds: KINDS.iter().filter(|k| k.available()).copied().collect(),
        properties: PROPERTIES,
        relations: RELATIONS,
    }
}
