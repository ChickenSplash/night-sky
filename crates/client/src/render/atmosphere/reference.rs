//! A slow, exact reference for the atmosphere's light, for tests only: light
//! followed backwards from the eye through the same air as `atmosphere.wgsl`,
//! scattering off the air and the ground any number of times (Monte Carlo,
//! with delta tracking for exact free paths), to measure multiple scattering
//! against. Each scattering event adds the starlight it receives directly, so
//! the light is kept by scattering order: order 1 is what the shader draws
//! today. One colour channel at a time, positions in planet radii, starlight
//! of 1 across its beam at the top of the air, a grey Lambertian ground.
use super::{
    Air, AirSettings, Preset,
    multiple::{multiple, seen, starlight, thinned},
    tables::{depths, extinction, sample},
};
use bevy::math::DVec3;
use std::f64::consts::PI;

/// Light gathered after scattering once, twice, and three or more times.
#[derive(Clone, Copy, Debug, Default)]
struct Orders([f64; 3]);

impl Orders {
    fn add(&mut self, order: usize, light: f64) {
        self.0[order.min(3) - 1] += light;
    }

    fn total(&self) -> f64 {
        self.0.iter().sum()
    }
}

/// SplitMix64: small, fast and deterministic.
struct Rng(u64);

impl Rng {
    /// Uniform in [0, 1).
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn rayleigh_phase(mu: f64) -> f64 {
    3.0 / (16.0 * PI) * (1.0 + mu * mu)
}

fn mie_phase(mu: f64, g: f64) -> f64 {
    let g2 = g * g;
    let denominator = (2.0 + g2) * (1.0 + g2 - 2.0 * g * mu).max(1e-6).powf(1.5);
    3.0 / (8.0 * PI) * (1.0 - g2) * (1.0 + mu * mu) / denominator
}

/// A direction at cosine `mu` from `axis`, at a random azimuth.
fn turn(axis: DVec3, mu: f64, rng: &mut Rng) -> DVec3 {
    let (a, b) = axis.any_orthonormal_pair();
    let phi = 2.0 * PI * rng.next();
    let sine = (1.0 - mu * mu).max(0.0).sqrt();
    (axis * mu + (a * phi.cos() + b * phi.sin()) * sine).normalize()
}

/// The cosine of a Rayleigh scattering angle: uniform, kept in proportion
/// to the phase function.
fn rayleigh_mu(rng: &mut Rng) -> f64 {
    loop {
        let mu = 2.0 * rng.next() - 1.0;
        if 2.0 * rng.next() < 1.0 + mu * mu {
            return mu;
        }
    }
}

/// The cosine of a Cornette-Shanks scattering angle: Henyey-Greenstein, kept
/// in proportion to the ratio of the two, `(1 + mu^2) / 2` of its peak.
fn mie_mu(g: f64, rng: &mut Rng) -> f64 {
    loop {
        let mu = if g.abs() < 1e-3 {
            2.0 * rng.next() - 1.0
        } else {
            let s = (1.0 - g * g) / (1.0 - g + 2.0 * g * rng.next());
            ((1.0 + g * g - s * s) / (2.0 * g)).clamp(-1.0, 1.0)
        };
        if 2.0 * rng.next() < 1.0 + mu * mu {
            return mu;
        }
    }
}

/// Distance along `d` from `p` to where it leaves the sphere of radius `r`.
fn leave(p: DVec3, d: DVec3, r: f64) -> f64 {
    let b = p.dot(d);
    -b + (b * b - p.length_squared() + r * r).max(0.0).sqrt()
}

/// Distance along `d` from `p` to the ground ahead, if it is met.
fn ground_ahead(p: DVec3, d: DVec3) -> Option<f64> {
    let b = p.dot(d);
    let disc = b * b - p.length_squared() + 1.0;
    if b >= 0.0 || disc < 0.0 {
        return None;
    }
    Some((-b - disc.sqrt()).max(0.0))
}

/// One air, one colour channel.
struct Reference {
    air: Air,
    channel: usize,
    top: f64,
    /// The most extinction anywhere in the air, for delta tracking.
    majorant: f64,
    /// Optical depth to the top of the air, for the starlight reaching each
    /// event (`tables.rs`, tested against a fine march).
    table: Vec<DVec3>,
    ground_albedo: f64,
}

impl Reference {
    fn new(air: Air, channel: usize, ground_albedo: f64) -> Self {
        let c = channel;
        let majorant = f64::from(
            air.rayleigh[c] + air.gas_absorption[c] + air.mie[c] + air.mie.w + air.absorption[c],
        );
        Self {
            air,
            channel,
            top: f64::from(air.top),
            majorant,
            table: depths(&air),
            ground_albedo,
        }
    }

    /// Rayleigh and Mie scattering, and extinction, at `height`.
    fn medium(&self, height: f64) -> (f64, f64, f64) {
        let (air, c) = (&self.air, self.channel);
        let above = height.max(0.0);
        let rayleigh = f64::from(air.rayleigh[c]) * (-above / f64::from(air.rayleigh.w)).exp();
        let mie = f64::from(air.mie[c]) * (-above / f64::from(air.shape.x)).exp();
        (rayleigh, mie, extinction(air, height)[c])
    }

    /// Starlight reaching `p` from direction `sun`, against 1 above the air.
    fn starlight(&self, p: DVec3, sun: DVec3) -> f64 {
        let b = p.dot(sun);
        let r2 = p.length_squared();
        if b < 0.0 && r2 - b * b < 1.0 {
            return 0.0;
        }
        let r = r2.sqrt();
        if r >= self.top {
            return 1.0;
        }
        (-sample(&self.table, self.top, r.max(1.0), b / r)[self.channel]).exp()
    }

    /// Radiance seen from `p` along `d`, by scattering order up to `most`.
    fn radiance(
        &self,
        mut p: DVec3,
        mut d: DVec3,
        sun: DVec3,
        most: usize,
        rng: &mut Rng,
    ) -> Orders {
        let mut light = Orders::default();
        // From outside, start where the ray enters the air.
        if p.length() >= self.top {
            let b = p.dot(d);
            let disc = b * b - p.length_squared() + self.top * self.top;
            if b >= 0.0 || disc < 0.0 {
                return light;
            }
            p += d * (-b - disc.sqrt());
        }
        let mut weight = 1.0;
        let mut order = 0;
        loop {
            let ground = ground_ahead(p, d);
            let limit = ground.unwrap_or_else(|| leave(p, d, self.top));
            let mut t = 0.0;
            let mut event = None;
            loop {
                t -= (1.0 - rng.next()).ln() / self.majorant;
                if t >= limit {
                    break;
                }
                let x = p + d * t;
                let (rayleigh, mie, extinction) = self.medium(x.length() - 1.0);
                if rng.next() * self.majorant < extinction {
                    event = Some((x, rayleigh, mie, extinction));
                    break;
                }
            }
            order += 1;
            match event {
                Some((x, rayleigh, mie, extinction)) => {
                    let scattering = rayleigh + mie;
                    let g = f64::from(self.air.shape.y);
                    let mu = d.dot(sun);
                    let phase = rayleigh * rayleigh_phase(mu) + mie * mie_phase(mu, g);
                    light.add(order, weight * phase / extinction * self.starlight(x, sun));
                    weight *= scattering / extinction;
                    let mu = if rng.next() * scattering < rayleigh {
                        rayleigh_mu(rng)
                    } else {
                        mie_mu(g, rng)
                    };
                    d = turn(d, mu, rng);
                    p = x;
                }
                None if ground.is_some() => {
                    let normal = (p + d * limit).normalize();
                    let facing = normal.dot(sun);
                    if facing > 0.0 {
                        let lit = facing * self.starlight(normal, sun);
                        light.add(order, weight * self.ground_albedo / PI * lit);
                    }
                    weight *= self.ground_albedo;
                    d = turn(normal, rng.next().sqrt(), rng);
                    p = normal * (1.0 + 1e-9);
                }
                None => break,
            }
            if order >= most {
                break;
            }
            // Russian roulette: faint paths end, or carry on brighter.
            if weight < 0.1 {
                if rng.next() * 0.1 > weight {
                    break;
                }
                weight = 0.1;
            }
        }
        light
    }

    /// Single scattering seen from `p` inside the air along `d`, marched
    /// finely, as the shader does without the opaque stop.
    fn single(&self, p: DVec3, d: DVec3, sun: DVec3) -> f64 {
        const STEPS: usize = 20_000;
        let limit = ground_ahead(p, d).unwrap_or_else(|| leave(p, d, self.top));
        let ds = limit / STEPS as f64;
        let g = f64::from(self.air.shape.y);
        let mu = d.dot(sun);
        let mut depth = 0.0;
        let mut light = 0.0;
        for i in 0..STEPS {
            let x = p + d * ((i as f64 + 0.5) * ds);
            let (rayleigh, mie, extinction) = self.medium(x.length() - 1.0);
            let phase = rayleigh * rayleigh_phase(mu) + mie * mie_phase(mu, g);
            light += (-depth - 0.5 * extinction * ds).exp() * phase * self.starlight(x, sun) * ds;
            depth += extinction * ds;
        }
        light
    }
}

/// The mean of `paths` estimates over every core, and the standard error
/// of its total.
fn mean(paths: usize, seed: u64, estimate: impl Fn(&mut Rng) -> Orders + Sync) -> (Orders, f64) {
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let each = paths.div_ceil(threads);
    let parts: Vec<(Orders, f64)> = std::thread::scope(|scope| {
        let estimate = &estimate;
        let handles: Vec<_> = (0..threads)
            .map(|i| {
                scope.spawn(move || {
                    let mut rng = Rng(seed ^ (i as u64 + 1).wrapping_mul(0xA24B_AED4_963E_E407));
                    let mut sum = Orders::default();
                    let mut squares = 0.0;
                    for _ in 0..each {
                        let one = estimate(&mut rng);
                        for k in 0..3 {
                            sum.0[k] += one.0[k];
                        }
                        squares += one.total() * one.total();
                    }
                    (sum, squares)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let n = (each * threads) as f64;
    let mut sum = Orders::default();
    let mut squares = 0.0;
    for (part, s) in parts {
        for k in 0..3 {
            sum.0[k] += part.0[k];
        }
        squares += s;
    }
    let mean = Orders(sum.0.map(|v| v / n));
    let variance = (squares / n - mean.total() * mean.total()).max(0.0);
    (mean, (variance / n).sqrt())
}

/// A point `km` above the ground, under a star `zenith_deg` from overhead,
/// and the star's direction.
fn place(radius_m: f64, km: f64, zenith_deg: f64) -> (DVec3, DVec3) {
    let z = zenith_deg.to_radians();
    (
        DVec3::Z * (1.0 + km * 1e3 / radius_m),
        DVec3::new(z.sin(), 0.0, z.cos()),
    )
}

/// The reference's single scattering is the shader's: for views across the
/// sky on Earth and Mars, from the ground and 10 km up, scattering once
/// matches a fine march within its statistical error.
#[test]
fn scattering_once_matches_the_march() {
    for (preset, radius) in [(Preset::Earth, 6.371e6), (Preset::Mars, 3.3895e6)] {
        let air = AirSettings::preset(preset).atmosphere().scaled(radius);
        for channel in [0, 2] {
            let reference = Reference::new(air, channel, 0.0);
            for km in [0.0, 10.0] {
                let (p, sun) = place(radius, km, 60.0);
                for d in [
                    DVec3::Z,
                    DVec3::new(0.98, 0.0, 0.2),
                    DVec3::new(-0.7, 0.3, 0.65),
                ] {
                    let d = d.normalize();
                    let marched = reference.single(p, d, sun);
                    let (got, error) =
                        mean(200_000, 7, |rng| reference.radiance(p, d, sun, 1, rng));
                    let got = got.total();
                    assert!(
                        (got - marched).abs() < 4.0 * error + 0.01 * marched,
                        "{preset:?} channel {channel}, {km} km, {d}: {got} ± {error} against {marched}"
                    );
                }
            }
        }
    }
}

/// The measurement multiple scattering is built against: for each preset,
/// the light reaching the ground and the sky's brightness, by scattering
/// order. Minutes; run with
/// `cargo test --release -p space-client atmosphere::reference -- --ignored --nocapture`.
#[test]
#[ignore = "measurement, minutes in release"]
fn measure_multiple_scattering() {
    const PATHS: usize = 40_000;
    const GROUND_ALBEDO: f64 = 0.3;
    let mut clear_cloud = AirSettings::preset(Preset::Venus).atmosphere();
    // Venus's sulphuric acid cloud barely absorbs (single scattering albedo
    // about 0.999); the preset's haze absorbs as Earth's does (0.9).
    clear_cloud.mie_absorption = clear_cloud.mie.x * 0.001;
    let airs = [
        (
            "Earth",
            AirSettings::preset(Preset::Earth).atmosphere(),
            6.371e6,
        ),
        (
            "Venus",
            AirSettings::preset(Preset::Venus).atmosphere(),
            6.0518e6,
        ),
        ("Venus, clear cloud", clear_cloud, 6.0518e6),
        (
            "Mars",
            AirSettings::preset(Preset::Mars).atmosphere(),
            3.3895e6,
        ),
        (
            "Titan",
            AirSettings::preset(Preset::Titan).atmosphere(),
            2.5747e6,
        ),
        (
            "Hydrogen giant",
            AirSettings::preset(Preset::HydrogenGiant).atmosphere(),
            6.9911e7,
        ),
    ];
    let rgb = |f: &dyn Fn(&Reference) -> (Orders, f64), air: Air| -> [(Orders, f64); 3] {
        [0, 1, 2].map(|c| f(&Reference::new(air, c, GROUND_ALBEDO)))
    };
    let number = |v: f64| {
        if v == 0.0 || v >= 1e-3 {
            format!("{v:.4}")
        } else {
            format!("{v:.0e}")
        }
    };
    let show = |label: &str, values: [(Orders, f64); 3]| {
        let row = |pick: &dyn Fn(&Orders) -> f64| values.map(|(o, _)| number(pick(&o))).join(" ");
        let once = row(&|o| o.0[0]);
        let all = row(&|o| o.total());
        let more = values.map(|(o, _)| match o.total() / o.0[0] {
            ratio if ratio < 1000.0 => format!("{ratio:.2}"),
            ratio if ratio.is_finite() => ">1000".into(),
            _ => "-".into(),
        });
        let error = values.map(|(o, e)| format!("{:.1}%", 100.0 * e / o.total().max(1e-30)));
        println!(
            "  {label:<38} once {once}   all {all}   all/once {}   error {}",
            more.join(" "),
            error.join(" ")
        );
    };
    for (name, atmosphere, radius) in airs {
        let air = atmosphere.scaled(radius);
        println!("{name} (red green blue; starlight 1, ground albedo {GROUND_ALBEDO})");
        for zenith in [0.0, 60.0, 85.0] {
            let (p, sun) = place(radius, 0.0, zenith);
            let direct = [0, 1, 2]
                .map(|c| Reference::new(air, c, GROUND_ALBEDO).starlight(p, sun) * sun.z)
                .map(number)
                .join(" ");
            println!("  ground, star {zenith}° from overhead: direct {direct}");
            // Light from the whole sky on the ground: pi times the mean
            // radiance of cosine-weighted directions.
            show(
                "    from the sky",
                rgb(
                    &|r| {
                        let (o, e) = mean(PATHS, 1, |rng| {
                            let d = turn(DVec3::Z, rng.next().sqrt(), rng);
                            r.radiance(p * (1.0 + 1e-9), d, sun, usize::MAX, rng)
                        });
                        (Orders(o.0.map(|v| v * PI)), e * PI)
                    },
                    air,
                ),
            );
        }
        for (label, zenith, d) in [
            ("sky overhead, star 60°", 60.0, DVec3::Z),
            (
                "sky at 5° up, towards the star at 60°",
                60.0,
                DVec3::new(0.996, 0.0, 0.087),
            ),
            ("sky overhead at twilight, star 95°", 95.0, DVec3::Z),
        ] {
            let (p, sun) = place(radius, 0.0, zenith);
            let p = p * (1.0 + 1e-9);
            show(
                label,
                rgb(
                    &|r| mean(PATHS, 2, |rng| r.radiance(p, d, sun, usize::MAX, rng)),
                    air,
                ),
            );
        }
        // From orbit, the brightness of the ground under a star overhead as
        // a reflectance (pi radiance over starlight): Venus's is about 0.8.
        let (_, sun) = place(radius, 0.0, 0.0);
        let p = DVec3::Z * 3.0;
        show(
            "reflectance from orbit, star overhead",
            rgb(
                &|r| {
                    let (o, e) = mean(PATHS, 3, |rng| {
                        r.radiance(p, -DVec3::Z, sun, usize::MAX, rng)
                    });
                    (Orders(o.0.map(|v| v * PI)), e * PI)
                },
                air,
            ),
        );
    }
}

/// Venus's haze absorption, against the light reaching its ground averaged
/// over the whole planet (real Venus about 2.5 to 3% of the mean starlight)
/// and its reflectance from orbit under an overhead star. Run as
/// `measure_multiple_scattering`.
#[test]
#[ignore = "measurement, a minute in release"]
fn measure_venus_haze_absorption() {
    const PATHS: usize = 20_000;
    let radius = 6.0518e6;
    for absorbed in [0.1, 0.01, 0.005, 0.003, 0.002, 0.001] {
        let air = AirSettings {
            haze_absorbed: absorbed,
            ..AirSettings::preset(Preset::Venus)
        }
        .atmosphere()
        .scaled(radius);
        let measured = [0, 1, 2].map(|c| {
            let reference = Reference::new(air, c, 0.3);
            // The planet's mean over its mean starlight (a quarter): twice
            // the integral of the ground's light over the star's cosine.
            let cosines: [f64; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];
            let ground: f64 = cosines
                .iter()
                .map(|&mu| {
                    let (p, sun) = place(radius, 0.0, mu.acos().to_degrees());
                    let (o, _) = mean(PATHS, 4, |rng| {
                        let d = turn(DVec3::Z, rng.next().sqrt(), rng);
                        reference.radiance(p * (1.0 + 1e-9), d, sun, usize::MAX, rng)
                    });
                    PI * o.total() + reference.starlight(p, sun) * mu
                })
                .sum::<f64>()
                / cosines.len() as f64
                * 2.0;
            let (o, _) = mean(PATHS, 5, |rng| {
                reference.radiance(DVec3::Z * 3.0, -DVec3::Z, DVec3::Z, usize::MAX, rng)
            });
            (ground, PI * o.total())
        });
        println!(
            "absorbed {absorbed}: ground {} of the mean starlight, reflectance {}",
            measured.map(|m| format!("{:.2}%", 100.0 * m.0)).join(" "),
            measured.map(|m| format!("{:.2}", m.1)).join(" "),
        );
    }
}

/// The multiple scattering table against the reference, for every preset:
/// the sky's radiance from the ground, the planet's reflectance from orbit
/// and the sky's light on the ground, all orders. Run as
/// `measure_multiple_scattering`.
#[test]
#[ignore = "measurement, a minute in release"]
fn measure_the_multiple_scattering_table() {
    const PATHS: usize = 40_000;
    let airs = [
        ("Earth", Preset::Earth, 6.371e6),
        ("Venus", Preset::Venus, 6.0518e6),
        ("Mars", Preset::Mars, 3.3895e6),
        ("Titan", Preset::Titan, 2.5747e6),
        ("Hydrogen giant", Preset::HydrogenGiant, 6.9911e7),
    ];
    let ratio = |table: DVec3, reference: [f64; 3]| {
        (0..3)
            .map(|c| {
                if reference[c] < 1e-9 {
                    "-".to_owned()
                } else {
                    format!("{:.2}", table[c] / reference[c])
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    for (name, preset, radius) in airs {
        let air = AirSettings::preset(preset).atmosphere().scaled(radius);
        let (whole, thin) = (depths(&air), depths(&thinned(&air)));
        let started = std::time::Instant::now();
        let table = multiple(&air, &thin);
        println!(
            "{name}: table built in {:.2} s; table over reference (red green blue)",
            started.elapsed().as_secs_f64()
        );
        let references = [0, 1, 2].map(|c| Reference::new(air, c, 0.3));
        for (label, zenith, d) in [
            ("sky overhead, star 60°", 60.0, DVec3::Z),
            (
                "sky at 5° up, towards the star at 60°",
                60.0,
                DVec3::new(0.996, 0.0, 0.087),
            ),
            (
                "sky at 30° up, away from the star at 30°",
                30.0,
                DVec3::new(-0.866, 0.0, 0.5),
            ),
            ("sky overhead at twilight, star 95°", 95.0, DVec3::Z),
        ] {
            let (p, sun) = place(radius, 0.0, zenith);
            let p = p * (1.0 + 1e-9);
            let d = d.normalize();
            let want = references.each_ref().map(|r| {
                mean(PATHS, 2, |rng| r.radiance(p, d, sun, usize::MAX, rng))
                    .0
                    .total()
            });
            let got = seen(&air, &thin, &table, p, d, sun);
            println!(
                "  {label:<42} {}   (reference green {:.2e})",
                ratio(got, want),
                want[1]
            );
        }
        for zenith in [0.0, 60.0] {
            let (_, sun) = place(radius, 0.0, zenith);
            let p = DVec3::Z * 3.0;
            let want = references.each_ref().map(|r| {
                mean(PATHS, 3, |rng| {
                    r.radiance(p, -DVec3::Z, sun, usize::MAX, rng)
                })
                .0
                .total()
            });
            let got = seen(&air, &thin, &table, p, -DVec3::Z, sun);
            println!(
                "  {:<42} {}   (reference green {:.2e})",
                format!("from orbit, star {zenith}°"),
                ratio(got, want),
                want[1]
            );
        }
        for zenith in [0.0, 60.0, 85.0] {
            let (p, sun) = place(radius, 0.0, zenith);
            let want = references.each_ref().map(|r| {
                PI * mean(PATHS, 1, |rng| {
                    let d = turn(DVec3::Z, rng.next().sqrt(), rng);
                    r.radiance(p * (1.0 + 1e-9), d, sun, usize::MAX, rng)
                })
                .0
                .total()
            });
            // The ground's radiance is its albedo over pi of all the light
            // on it; less the starlight, that from the sky.
            let direct =
                starlight(&whole, f64::from(air.top), p * (1.0 + 1e-9), sun) * sun.z.max(0.0);
            let got = table.ground_at(sun.z) * PI / 0.3 - direct;
            println!(
                "  {:<42} {}   (reference green {:.2e})",
                format!("sky light on the ground, star {zenith}°"),
                ratio(got, want),
                want[1]
            );
        }
    }
}

/// The ground row now consumed by body.wgsl, against all-order path tracing
/// of a surface viewed from immediately above it, with no view-path dimming.
/// Run with `cargo test -p space-client measure_ground_lighting -- --ignored --nocapture`.
#[test]
#[ignore = "measurement, tens of seconds"]
fn measure_ground_lighting() {
    const PATHS: usize = 40_000;
    for (preset, radius) in [
        (Preset::Earth, 6.371e6),
        (Preset::Venus, 6.0518e6),
        (Preset::Mars, 3.3895e6),
        (Preset::Titan, 2.5747e6),
    ] {
        let air = AirSettings::preset(preset).atmosphere().scaled(radius);
        let thin = depths(&thinned(&air));
        let table = multiple(&air, &thin);
        for channel in 0..3 {
            let reference = Reference::new(air, channel, f64::from(air.ground[channel]));
            for zenith in [30.0, 85.0] {
                let (p, sun) = place(radius, 0.0, zenith);
                let (estimate, error) = mean(PATHS, 7, |rng| {
                    reference.radiance(p * (1.0 + 1e-9), -DVec3::Z, sun, usize::MAX, rng)
                });
                let want = estimate.total();
                let got = table.ground_at(sun.z)[channel];
                println!(
                    "{preset:?} channel {channel}, star {zenith}°: table {got:.5e}, reference {want:.5e} ± {error:.2e}, ratio {:.3}",
                    got / want.max(1e-30),
                );
                // The transport approximation and isotropic later scattering
                // are least accurate for forward dust at low sun. Also allow
                // Monte Carlo uncertainty rather than asserting on noise.
                assert!(
                    (got - want).abs() <= 0.35 * want + 4.0 * error + 1e-9,
                    "{preset:?} channel {channel}, star {zenith}°: {got} vs {want} ± {error}",
                );
            }
        }
    }
}
