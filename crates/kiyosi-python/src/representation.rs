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

pub(crate) fn times(py: Python<'_>, items: &[kiyosi::Timestamp]) -> PyResult<String> {
    preview(items, |&time| {
        Ok(python_timestamp(py, time)?.repr()?.to_string())
    })
}

fn numbers(items: &[f64]) -> PyResult<String> {
    preview(items, |value| Ok(format!("{value:?}")))
}

pub(crate) fn curve(py: Python<'_>, value: &kiyosi::RateTermStructure) -> PyResult<String> {
    let reference = value.reference_time().map_or_else(
        || Ok("None".into()),
        |time| Ok::<_, PyErr>(python_timestamp(py, time)?.repr()?.to_string()),
    )?;
    if let Some(rate) = value.exact_flat_rate().filter(|_| value.nodes().is_empty()) {
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

pub(crate) fn model(
    value: &Bound<'_, PyAny>,
    risk_free: &kiyosi::RateTermStructure,
    dividend: &kiyosi::RateTermStructure,
) -> PyResult<String> {
    let py = value.py();
    format(
        value,
        [
            ("risk_free_rate", Some(curve(py, risk_free)?)),
            ("dividend_yield", Some(curve(py, dividend)?)),
        ],
    )
}

pub(crate) fn surface(py: Python<'_>, value: &kiyosi::LocalVolatilitySurface) -> PyResult<String> {
    Ok(format!(
        "LocalVolatilitySurface(times={}, spots={}, values={})",
        times(py, value.times())?,
        numbers(value.spots())?,
        preview(value.values(), numbers)?
    ))
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
    Ok(format!(
        "TradingCalendar(kind='{}', trading_days_per_year={})",
        crate::calendar::builtin_name(value)?,
        value.trading_days_per_year()
    ))
}

pub(crate) fn calendar_preview(
    py: Python<'_>,
    value: &kiyosi::TradingCalendar,
) -> PyResult<Option<String>> {
    if value == &kiyosi::TradingCalendar::default() {
        Ok(None)
    } else {
        calendar(py, value).map(Some)
    }
}

pub(crate) fn observation(
    py: Python<'_>,
    value: &kiyosi::AutocallableObservation,
) -> PyResult<String> {
    let (kind, field, coupon) = match value.kind() {
        kiyosi::AutocallableObservationKind::Snowball => {
            ("snowball", "coupon_rate", value.coupon_rate())
        }
        kiyosi::AutocallableObservationKind::Phoenix => (
            "phoenix",
            "coupon_barrier_level",
            value.coupon_barrier_level(),
        ),
    };
    Ok(format!(
        "AutocallableObservation.{kind}({}, {:?}, {field}={:?})",
        python_timestamp(py, value.time())?.repr()?,
        value.knock_out_level(),
        coupon.unwrap_or_default(),
    ))
}

pub(crate) fn observations(
    py: Python<'_>,
    values: &[kiyosi::AutocallableObservation],
) -> PyResult<String> {
    preview(values, |value| observation(py, value))
}

pub(crate) fn format(
    value: &Bound<'_, PyAny>,
    fields: impl IntoIterator<Item = (&'static str, Option<String>)>,
) -> PyResult<String> {
    let py = value.py();
    let previews = PyDict::new(py);
    for (name, preview) in fields {
        if let Some(preview) = preview {
            previews.set_item(name, preview)?;
        }
    }
    PyModule::import(py, "kiyosi._serialization")?
        .getattr("_repr")?
        .call1((value, previews))?
        .extract()
}
