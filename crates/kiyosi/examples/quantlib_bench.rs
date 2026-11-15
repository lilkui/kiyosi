//! Native price-request batches for the optional `QuantLib` comparison suite.

#[path = "quantlib_bench/american_monte_carlo.rs"]
mod american_monte_carlo;
#[path = "quantlib_bench/asian.rs"]
mod asian;
#[path = "quantlib_bench/barriers.rs"]
mod barriers;
#[path = "quantlib_bench/common.rs"]
mod common;
#[path = "quantlib_bench/crr.rs"]
mod crr;
#[path = "quantlib_bench/digital.rs"]
mod digital;
#[path = "quantlib_bench/european.rs"]
mod european;
#[path = "quantlib_bench/quadrature.rs"]
mod quadrature;

#[path = "quantlib_bench/monte_carlo.rs"]
mod monte_carlo;

#[path = "quantlib_bench/fd.rs"]
mod fd;
#[path = "quantlib_bench/local_vol.rs"]
mod local_vol;

#[path = "quantlib_bench/heston.rs"]
mod heston;

use std::{hint::black_box, time::Instant};

use common::Request;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() == 2 && std::env::args().nth(1).as_deref() == Some("--info") {
        println!(
            "{{\"library\":\"kiyosi\",\"version\":\"{}\",\"families\":[\"european_bsm\",\"digital_bsm\",\"geometric_asian_bsm\",\"arithmetic_asian_bsm\",\"fd_bsm\",\"fd_local_vol\",\"single_barrier_bsm\",\"binary_barrier_bsm\",\"touch_bsm\",\"double_barrier_bsm\",\"double_touch_bsm\",\"quadrature_vanilla\",\"quadrature_digital\",\"crr_vanilla\",\"crr_barrier\",\"mc_european_bsm\",\"mc_barrier_bsm\",\"mc_american_bsm\",\"heston_analytic\",\"heston_fd_european\",\"heston_fd_american\"]}}",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    let mut request = Request::parse()?;
    let family = request.take("family")?;
    let iterations: u64 = request.number("iterations")?;
    if iterations == 0 {
        return Err("iterations must be positive".into());
    }
    let price = match family.as_str() {
        "mc_american_bsm" => american_monte_carlo::prepare(&mut request)?,
        "mc_european_bsm" => monte_carlo::prepare(&mut request, false)?,
        "mc_barrier_bsm" => monte_carlo::prepare(&mut request, true)?,

        "fd_bsm" => fd::prepare(&mut request)?,
        "fd_local_vol" => local_vol::prepare(&mut request)?,
        "european_bsm" => european::prepare(&mut request)?,
        "quadrature_vanilla" => quadrature::prepare(&mut request, false)?,
        "quadrature_digital" => quadrature::prepare(&mut request, true)?,
        "crr_vanilla" => crr::prepare(&mut request, false)?,
        "crr_barrier" => crr::prepare(&mut request, true)?,
        "digital_bsm" => digital::prepare(&mut request)?,
        "geometric_asian_bsm" => asian::prepare(&mut request, false)?,
        "arithmetic_asian_bsm" => asian::prepare(&mut request, true)?,
        "single_barrier_bsm" | "binary_barrier_bsm" | "touch_bsm" | "double_barrier_bsm"
        | "double_touch_bsm" => barriers::prepare(&mut request, &family)?,
        "heston_analytic" | "heston_fd_european" | "heston_fd_american" => {
            heston::prepare(&mut request, &family)?
        }
        _ => return Err(format!("unknown family: {family}").into()),
    };
    request.finish()?;
    let mut result = 0.0;
    for _ in 0..3 {
        black_box(black_box(&price)()?);
    }
    let start = Instant::now();
    for _ in 0..iterations {
        result = black_box(black_box(&price)()?);
    }
    let elapsed = start.elapsed().as_nanos();
    if !result.is_finite() {
        return Err("nonfinite native price".into());
    }
    println!(
        "{{\"price\":{result:.17},\"elapsed_ns\":{elapsed},\"iterations\":{iterations},\"warmup_iterations\":3}}"
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("kiyosi benchmark: {error}");
        std::process::exit(1);
    }
}
