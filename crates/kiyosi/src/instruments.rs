pub(crate) mod accumulator;
pub(crate) mod asian;
pub(crate) mod autocallable;
pub(crate) mod barrier;
pub(crate) mod binary_barrier;
pub(crate) mod double_barrier;
pub(crate) mod double_touch;
pub(crate) mod monitoring;
pub(crate) mod range_accrual;
pub(crate) mod vanilla;

pub(crate) fn validate_history_replacement(
    known: bool,
    supplied: bool,
    events_changed: bool,
    field: &str,
) -> crate::Result<()> {
    if known && !supplied && events_changed {
        return Err(crate::Error::InvalidInput(
            format!("changing historical observation terms requires explicit {field}").into(),
        ));
    }
    Ok(())
}
