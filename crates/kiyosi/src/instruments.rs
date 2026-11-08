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

#[expect(
    clippy::option_option,
    reason = "Omission, clearing and an explicit timestamp are distinct replacement choices."
)]
pub(crate) fn replacement_history_time(
    current: Option<crate::Timestamp>,
    replacement: Option<Option<crate::Timestamp>>,
    cleared: bool,
) -> Option<crate::Timestamp> {
    replacement.unwrap_or(if cleared { None } else { current })
}

pub(crate) fn validate_history_replacement(
    known: bool,
    supplied: bool,
    events_changed: bool,
    field: &str,
) -> crate::Result<()> {
    if known && !supplied && events_changed {
        return Err(crate::Error::StaleHistory(
            format!("changing historical observations or history_time requires explicit {field}")
                .into(),
        ));
    }
    Ok(())
}

pub(crate) fn historical_times(
    times: &[crate::Timestamp],
    history_time: Option<crate::Timestamp>,
) -> &[crate::Timestamp] {
    &times[..history_time.map_or(times.len(), |at| times.partition_point(|&time| time < at))]
}

pub(crate) fn validate_history_redating(
    option: &impl crate::OptionContract,
    time: Option<crate::Timestamp>,
    known: bool,
) -> crate::Result<()> {
    validate_history_time_bounds(option, time)?;
    validate_history_replacement(
        known,
        false,
        option.history_time().is_some() && time.is_some() && option.history_time() != time,
        "history payload",
    )
}

pub(crate) fn validate_history_time(
    option: &impl crate::OptionContract,
    known: bool,
    context: &crate::PricingContext,
    continuous: bool,
) -> crate::Result<()> {
    if !known {
        return Ok(());
    }
    let time = option.history_time().ok_or_else(|| {
        crate::Error::MissingHistory("supplied history requires history_time".into())
    })?;
    validate_history_time_bounds(option, Some(time))?;
    let valuation = context.history_anchor.unwrap_or(context.valuation_time());
    if time > valuation {
        return Err(crate::Error::InvalidInput(
            "history_time cannot follow valuation".into(),
        ));
    }
    if time != valuation {
        let before = time.min(valuation);
        let after = time.max(valuation).add_micros(-1)?;
        if continuous || !option.time_stencil_available(context, before, after) {
            return Err(crate::Error::StaleHistory(
                "history_time does not cover this valuation; replace and date the history".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_history_time_bounds(
    option: &impl crate::OptionContract,
    time: Option<crate::Timestamp>,
) -> crate::Result<()> {
    if time.is_some_and(|time| time < option.effective_time() || time > option.expiry_time()) {
        return Err(crate::Error::InvalidInput(
            "history_time is outside contract life".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_history_snapshot(
    option: &impl crate::OptionContract,
    known: bool,
) -> crate::Result<()> {
    if known && option.history_time().is_none() {
        return Err(crate::Error::MissingHistory(
            "supplied history requires history_time".into(),
        ));
    }
    validate_history_time_bounds(option, option.history_time())
}
