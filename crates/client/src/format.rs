//! Human-readable text for physical quantities, counts and dates.
use space_simulation::{AU_METRES, LY_METRES};

/// Use larger units only once the value reaches one whole unit.
pub fn speed_label(metres_per_second: f64) -> String {
    let (value, unit) = if metres_per_second >= LY_METRES {
        (metres_per_second / LY_METRES, "ly/s")
    } else if metres_per_second >= AU_METRES {
        (metres_per_second / AU_METRES, "AU/s")
    } else if metres_per_second >= 1000.0 {
        (metres_per_second / 1000.0, "km/s")
    } else {
        (metres_per_second, "m/s")
    };
    format!("{value:.2} {unit}")
}

const JUPITER_MASSES: f64 = 317.83;

pub fn mass_text(earths: f64) -> String {
    if earths >= 0.1 * JUPITER_MASSES {
        format!("{:.2} Jupiter masses", earths / JUPITER_MASSES)
    } else if earths < 0.1 {
        format!("{earths:.4} Earth masses")
    } else {
        format!("{earths:.2} Earth masses")
    }
}

pub fn period_text(days: f64) -> String {
    let years = days / 365.25;
    if years >= 1_000.0 {
        format!("{} years", grouped(years.round() as usize))
    } else if days >= 730.0 {
        format!("{years:.1} years")
    } else {
        format!("{days:.1} days")
    }
}

/// A distance in AU, with more places when small.
pub fn au_text(au: f64) -> String {
    if au < 10.0 {
        format!("{au:.3} AU")
    } else {
        format!("{} AU", grouped(au.round() as usize))
    }
}

/// A distance in km, grouped in thousands, with a decimal when small.
pub fn km_text(km: f64) -> String {
    if km.abs() < 1_000.0 {
        format!("{km:.1} km")
    } else {
        let sign = if km < 0.0 { "-" } else { "" };
        format!("{sign}{} km", grouped(km.abs().round() as usize))
    }
}

pub fn rotation_text(hours: f64) -> String {
    if hours >= 48.0 {
        period_text(hours / 24.0)
    } else if hours >= 2.0 {
        format!("{hours:.1} hours")
    } else {
        format!("{:.0} minutes", hours * 60.0)
    }
}

pub fn tilt_text(radians: f64) -> String {
    let degrees = radians.to_degrees();
    format!(
        "{degrees:.1}°{}",
        if degrees > 90.0 { " · retrograde" } else { "" }
    )
}

pub fn star_radius_text(solar: f64) -> String {
    if solar < 0.05 {
        format!("{:.0} km", solar * 695_700.0)
    } else {
        format!("{solar:.2} solar radii")
    }
}

pub fn luminosity_text(solar: f64) -> String {
    if solar < 0.01 {
        format!("{solar:.1e}")
    } else if solar < 10.0 {
        format!("{solar:.3}")
    } else {
        format!("{solar:.0}")
    }
}

/// Day, month and year of a Unix time in milliseconds (UTC).
pub fn civil_date(ms: i64) -> (i64, &'static str, i64) {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Howard Hinnant's days-to-civil algorithm.
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (day, MONTHS[month as usize - 1], year)
}

pub fn due_short(ms: i64) -> String {
    let days = (ms - crate::network::now_ms()) as f64 / 86_400_000.0;
    if days.abs() < 1.0 {
        "Today".into()
    } else if days < 0.0 {
        format!("{:.0}d late", -days)
    } else if days > 45.0 {
        let (day, month, _) = civil_date(ms);
        format!("{day} {month}")
    } else {
        format!("in {days:.0}d")
    }
}

pub fn due_text(ms: i64) -> String {
    let (day, month, year) = civil_date(ms);
    format!("{day} {month} {year} · {}", due_short(ms).to_lowercase())
}

/// `17,012,345`.
pub fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `51.2M`, `830k`.
pub fn si(n: f64) -> String {
    match n {
        n if n >= 1e9 => format!("{:.1}G", n / 1e9),
        n if n >= 1e6 => format!("{:.1}M", n / 1e6),
        n if n >= 1e3 => format!("{:.0}k", n / 1e3),
        n => format!("{n:.0}"),
    }
}
