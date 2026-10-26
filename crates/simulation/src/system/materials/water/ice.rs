//! Density from IAPWS R10-06(2009), equation 1 and tables 2-4, in SI.
//! Coefficients attributed to IAPWS. Only pressure-dependent Gibbs terms
//! contribute to specific volume; entropy reference constants cancel.

pub(super) fn density_kg_m3(pressure_pa: f64, temperature_k: f64) -> f64 {
    let pt = 611.657;
    let q = (pressure_pa - 101325.0) / pt;
    let g0_p = (0.655022213658955 - 2.0 * 0.189369929326131e-7 * q
        + 3.0 * 0.339746123271053e-14 * q.powi(2)
        - 4.0 * 0.556464869058991e-21 * q.powi(3))
        / pt;
    let r2_p = (
        (-0.557107698030123e-4 + 2.0 * 0.234801409215913e-10 * q) / pt,
        (0.464578634580806e-4 - 2.0 * 0.285651142904972e-10 * q) / pt,
    );
    let (a, b) = (0.337315741065416, 0.335449415919309);
    let tau = temperature_k / 273.16;
    let minus = z_log_z(a - tau, b);
    let plus = z_log_z(a + tau, b);
    let centre = z_log_z(a, b);
    let bracket = (
        minus.0 + plus.0 - 2.0 * centre.0 - tau.powi(2) * a / (a * a + b * b),
        minus.1 + plus.1 - 2.0 * centre.1 + tau.powi(2) * b / (a * a + b * b),
    );
    let volume = g0_p + 273.16 * (r2_p.0 * bracket.0 - r2_p.1 * bracket.1);
    1.0 / volume
}

/// z ln(z), principal complex logarithm, evaluated as two real components.
/// This avoids a complex-number dependency for this one closed-form derivative.
fn z_log_z(real: f64, imaginary: f64) -> (f64, f64) {
    let log_modulus = real.hypot(imaginary).ln();
    let argument = imaginary.atan2(real);
    (
        real * log_modulus - imaginary * argument,
        imaginary * log_modulus + real * argument,
    )
}
