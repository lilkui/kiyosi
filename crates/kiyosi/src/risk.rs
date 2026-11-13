pub(crate) mod analytics;
pub(crate) mod coupon;
pub(crate) mod implied;
pub(crate) mod numerical;

fn validate_solver_bounds(lower: f64, upper: f64, max_iterations: i32) -> crate::Result<()> {
    if lower >= upper {
        return Err(crate::Error::InvalidInput(
            "upper_bound must exceed lower_bound".into(),
        ));
    }
    if max_iterations <= 0 {
        return Err(crate::Error::InvalidInput(
            "max_iterations must be positive".into(),
        ));
    }
    Ok(())
}
