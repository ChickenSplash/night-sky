//! Human-readable text for physical quantities, counts and dates.
use space_simulation::{AU_METRES, LY_METRES};

/// Fixed-point readout: comma thousands groups, decimal point, retained precision.
pub fn number(value: f64, decimals: usize) -> String {
    group_decimal(&format!("{value:.decimals$}"))
}

pub fn signed_number(value: f64, decimals: usize) -> String {
    format!(
        "{}{}",
        if value.is_sign_negative() { "-" } else { "+" },
        number(value.abs(), decimals)
    )
}

fn group_decimal(text: &str) -> String {
    let (sign, digits) = text.strip_prefix('-').map_or(("", text), |s| ("-", s));
    let (integer, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if !integer.bytes().all(|c| c.is_ascii_digit()) {
        return text.into();
    }
    let mut out = String::from(sign);
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if digits.contains('.') {
        out.push('.');
        out.push_str(fraction);
    }
    out
}

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
    format!("{} {unit}", number(value, 2))
}

/// A camera range: metres, then km, AU from 0.1 AU, and ly from 0.01 ly.
pub fn distance_label(metres: f64) -> String {
    if metres >= 0.01 * LY_METRES {
        format!("{} ly", number(metres / LY_METRES, 3))
    } else if metres >= 0.1 * AU_METRES {
        format!("{} AU", number(metres / AU_METRES, 3))
    } else if metres >= 1000.0 {
        km_text(metres / 1000.0)
    } else {
        format!("{} m", number(metres, 1))
    }
}

const JUPITER_MASSES: f64 = 317.83;

pub fn mass_text(earths: f64) -> String {
    if earths >= 0.1 * JUPITER_MASSES {
        format!("{} Jupiter masses", number(earths / JUPITER_MASSES, 2))
    } else if earths < 0.1 {
        format!("{} Earth masses", number(earths, 4))
    } else {
        format!("{} Earth masses", number(earths, 2))
    }
}

pub fn period_text(days: f64) -> String {
    let years = days / 365.25;
    if years >= 1_000.0 {
        format!("{} years", grouped(years.round() as usize))
    } else if days >= 730.0 {
        format!("{} years", number(years, 1))
    } else {
        format!("{} days", number(days, 1))
    }
}

/// A span of simulation time: seconds, minutes, hours, then days or years.
pub fn duration_text(seconds: f64) -> String {
    if seconds < 120.0 {
        format!("{} s", number(seconds, 0))
    } else if seconds < 7_200.0 {
        format!("{} min", number(seconds / 60.0, 1))
    } else if seconds < 2.0 * 86_400.0 {
        format!("{} hours", number(seconds / 3_600.0, 2))
    } else {
        period_text(seconds / 86_400.0)
    }
}

/// A distance in AU, with more places when small.
pub fn au_text(au: f64) -> String {
    if au < 10.0 {
        format!("{} AU", number(au, 3))
    } else {
        format!("{} AU", grouped(au.round() as usize))
    }
}

/// A distance in km, grouped in thousands, with a decimal when small.
pub fn km_text(km: f64) -> String {
    if km.abs() < 1_000.0 {
        format!("{} km", number(km, 1))
    } else {
        format!("{} km", number(km, 0))
    }
}

pub fn rotation_text(hours: f64) -> String {
    if hours >= 48.0 {
        period_text(hours / 24.0)
    } else if hours >= 2.0 {
        format!("{} hours", number(hours, 1))
    } else {
        format!("{} minutes", number(hours * 60.0, 0))
    }
}

pub fn tilt_text(radians: f64) -> String {
    let degrees = radians.to_degrees();
    format!(
        "{degrees:.1}°{}",
        if degrees > 90.0 { " · retrograde" } else { "" }
    )
}

/// A body's flattening: `1/298` when slight, `0.065` when marked.
pub fn flattening_text(flattening: f64) -> String {
    if flattening < 1e-7 {
        "None".into()
    } else if flattening < 0.01 {
        format!("1/{}", grouped((1.0 / flattening).round() as usize))
    } else {
        format!("{flattening:.3}")
    }
}

pub fn star_radius_text(solar: f64) -> String {
    if solar < 0.05 {
        format!("{} km", number(solar * 695_700.0, 0))
    } else {
        format!("{} solar radii", number(solar, 2))
    }
}

pub fn luminosity_text(solar: f64) -> String {
    if solar < 0.01 {
        format!("{solar:.1e}")
    } else if solar < 10.0 {
        format!("{solar:.3}")
    } else {
        number(solar, 0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_readouts_group_only_the_integer_part_and_keep_precision() {
        assert_eq!(number(21580.174, 3), "21,580.174");
        assert_eq!(number(1000000.125, 3), "1,000,000.125");
        assert_eq!(number(-21580.174, 3), "-21,580.174");
        assert_eq!(number(999.9999, 3), "1,000.000");
        assert_eq!(number(0.000012, 6), "0.000012");
        assert_eq!(number(1234.0, 0), "1,234");
        assert_eq!(number(f64::NAN, 3), "NaN");
        assert_eq!(number(f64::INFINITY, 3), "inf");
        assert_eq!(signed_number(21580.174, 3), "+21,580.174");
    }
}
