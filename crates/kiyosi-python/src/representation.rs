use crate::conversions::python_timestamp;
use pyo3::prelude::*;
use pyo3::types::PyDict;

fn preview<T>(
    items: impl IntoIterator<Item = T>,
    mut render: impl FnMut(T) -> PyResult<String>,
) -> PyResult<String> {
    let mut items = items.into_iter();
    let mut values = items
        .by_ref()
        .take(6)
        .map(&mut render)
        .collect::<PyResult<Vec<_>>>()?;
    if items.next().is_some() {
        values.push("...".into());
    }
    Ok(format!("[{}]", values.join(", ")))
}

fn times(py: Python<'_>, items: &[kiyosi::Timestamp]) -> PyResult<String> {
    preview(items, |&time| {
        Ok(python_timestamp(py, time)?.repr()?.to_string())
    })
}

fn numbers(items: &[f64]) -> PyResult<String> {
    preview(items, |value| Ok(format!("{value:?}")))
}

fn curve(py: Python<'_>, value: &kiyosi::RateTermStructure) -> PyResult<String> {
    let reference = value.reference_time().map_or_else(
        || Ok("None".into()),
        |time| Ok::<_, PyErr>(python_timestamp(py, time)?.repr()?.to_string()),
    )?;
    if let Some(rate) = value.flat_rate().filter(|_| value.nodes().is_empty()) {
        if let Some(end) = value.max_time() {
            return Ok(format!(
                "RateTermStructure.bounded_flat(rate={rate:?}, reference_time={reference}, max_time={})",
                python_timestamp(py, end)?.repr()?
            ));
        }
        return Ok(format!(
            "RateTermStructure.flat(rate={rate:?}, reference_time={reference})"
        ));
    }
    Ok(format!(
        "RateTermStructure(reference_time={reference}, times={}, factors={}).shifted({:?})",
        times(py, value.nodes())?,
        numbers(value.base_factors())?,
        value.parallel_shift()
    ))
}

fn surface(py: Python<'_>, value: &kiyosi::LocalVolatilitySurface) -> PyResult<String> {
    Ok(format!(
        "LocalVolatilitySurface(times={}, spots={}, values={})",
        times(py, value.times())?,
        numbers(value.spots())?,
        preview(value.values(), numbers)?
    ))
}

fn model(py: Python<'_>, value: &kiyosi::ModelParameters) -> PyResult<String> {
    let rates = format!(
        "risk_free_rate={}, dividend_yield={}",
        curve(py, value.risk_free_curve())?,
        curve(py, value.dividend_curve())?
    );
    Ok(match value {
        kiyosi::ModelParameters::BlackScholesMerton(m) => format!(
            "BlackScholesMertonParameters({rates}, volatility={:?})",
            m.volatility()
        ),
        kiyosi::ModelParameters::LocalVolatility(m) => format!(
            "LocalVolatilityParameters({rates}, surface={})",
            surface(py, m.surface())?
        ),
        kiyosi::ModelParameters::Heston(m) => format!(
            "HestonParameters({rates}, initial_variance={:?}, long_run_variance={:?}, mean_reversion_speed={:?}, vol_of_vol={:?}, correlation={:?})",
            m.initial_variance(),
            m.long_run_variance(),
            m.mean_reversion_speed(),
            m.vol_of_vol(),
            m.correlation()
        ),
    })
}

pub(crate) fn calendar(py: Python<'_>, value: &kiyosi::TradingCalendar) -> PyResult<String> {
    if let Some((base, closed, open)) = value.overrides() {
        let dates = |values: &[kiyosi::Date]| {
            preview(values, |&date| {
                Ok(crate::conversions::python_date(py, date)?
                    .repr()?
                    .to_string())
            })
        };
        return Ok(format!(
            "TradingCalendar(kind='custom', trading_days_per_year={}, base={}, closed_dates={}, open_dates={})",
            value.trading_days_per_year(),
            calendar(py, base)?,
            dates(closed)?,
            dates(open)?
        ));
    }
    for (name, base) in [
        ("weekdays", kiyosi::TradingCalendar::weekdays()),
        ("all_days", kiyosi::TradingCalendar::all_days()),
        ("sse", kiyosi::TradingCalendar::sse()),
    ] {
        let base = crate::error::domain(kiyosi::TradingCalendar::custom(
            base,
            vec![],
            vec![],
            Some(value.trading_days_per_year()),
        ))?;
        if *value == base {
            return Ok(format!(
                "TradingCalendar(kind='{name}', trading_days_per_year={})",
                value.trading_days_per_year()
            ));
        }
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "predicate calendars have no Python representation",
    ))
}

#[pyfunction]
pub(crate) fn _repr_previews<'py>(
    py: Python<'py>,
    value: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    macro_rules! direct {
        ($class:ty, $render:expr) => {
            if let Ok(v) = value.extract::<PyRef<'_, $class>>() {
                result.set_item("__repr__", $render(&v.0)?)?;
                return Ok(result);
            }
        };
    }
    direct!(
        crate::local_volatility::LocalVolatilitySurface,
        |v| surface(py, v)
    );
    direct!(crate::market::RateTermStructure, |v| curve(py, v));
    direct!(crate::calendar::TradingCalendar, |v| calendar(py, v));
    direct!(
        crate::schedule::ObservationSchedule,
        |v: &kiyosi::ObservationSchedule| Ok::<_, PyErr>(format!(
            "ObservationSchedule(times={})",
            times(py, v.times())?
        ))
    );
    // Borrow model storage; constructing temporary model wrappers would clone entire surfaces.
    if let Ok(v) = value.extract::<PyRef<'_, crate::market::PricingContext>>() {
        result.set_item(
            "__repr__",
            format!(
                "PricingContext(model_parameters={}, spot_price={:?}, valuation_time={})",
                model(py, v.0.model_parameters())?,
                v.0.spot_price(),
                python_timestamp(py, v.0.valuation_time())?.repr()?
            ),
        )?;
        return Ok(result);
    }
    macro_rules! rates {
        ($class:ty) => {
            if let Ok(v) = value.extract::<PyRef<'_, $class>>() {
                result.set_item("risk_free_rate", curve(py, v.0.risk_free_curve())?)?;
                result.set_item("dividend_yield", curve(py, v.0.dividend_curve())?)?;
            }
        };
    }
    rates!(crate::market::BlackScholesMertonParameters);
    rates!(crate::heston::HestonParameters);
    rates!(crate::local_volatility::LocalVolatilityParameters);
    if let Ok(v) = value.extract::<PyRef<'_, crate::local_volatility::LocalVolatilityParameters>>()
    {
        result.set_item("surface", surface(py, v.0.surface())?)?;
    }
    macro_rules! monitoring {
        ($($class:ty),+ $(,)?) => {$(
            if let Ok(v) = value.extract::<PyRef<'_, $class>>() {
                if v.0.calendar() != &kiyosi::TradingCalendar::default() {
                    result.set_item("calendar", calendar(py, v.0.calendar())?)?;
                }
                result.set_item("observation_times", times(py, v.0.observation_times())?)?;
            }
        )+};
    }
    monitoring!(
        crate::instruments::barrier::BarrierOption,
        crate::instruments::double_barrier::DoubleBarrierOption,
        crate::instruments::binary_barrier::BinaryBarrierOption,
        crate::instruments::binary_barrier::TouchOption,
        crate::instruments::double_touch::DoubleTouchOption
    );
    if let Ok(v) = value.extract::<PyRef<'_, crate::instruments::accumulator::Accumulator>>() {
        if v.0.calendar() != &kiyosi::TradingCalendar::default() {
            result.set_item("calendar", calendar(py, v.0.calendar())?)?;
        }
        result.set_item(
            "knock_out_observation_times",
            v.0.knock_out_observation_times()
                .map_or_else(|| Ok("None".into()), |v| times(py, v))?,
        )?;
    }
    macro_rules! notes {
        ($($class:ty),+ $(,)?) => {$(
            if let Ok(v) = value.extract::<PyRef<'_, $class>>() {
                if v.0.calendar() != &kiyosi::TradingCalendar::default() {
                    result.set_item("calendar", calendar(py, v.0.calendar())?)?;
                }
                result.set_item("observations", preview(v.0.observations(), |r| Ok(format!(
                    "AutocallableObservation({}, {:?}, coupon_rate={:?}, coupon_barrier_level={:?})",
                    python_timestamp(py, r.time)?.repr()?, r.knock_out_level, r.coupon_rate, r.coupon_barrier_level)))?)?;
            }
        )+};
    }
    notes!(
        crate::instruments::autocallable::SnowballOption,
        crate::instruments::autocallable::BinarySnowballOption,
        crate::instruments::autocallable::TernarySnowballOption,
        crate::instruments::autocallable::PhoenixOption
    );
    Ok(result)
}
