use crate::conversions::{check_changes, date, integer, python_date, real};
use crate::error::domain;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

fn history(value: &Bound<'_, PyAny>) -> PyResult<Option<i32>> {
    if value.is_none() {
        Ok(None)
    } else {
        integer(value).map(Some)
    }
}

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct RangeAccrualOption(pub core::RangeAccrualOption);

#[pymethods]
impl RangeAccrualOption {
    #[new]
    #[pyo3(signature = (*, notional, annual_coupon_rate, lower_bound, upper_bound, effective_date, expiry_date, realized_hit_count=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        notional: &Bound<'_, PyAny>,
        annual_coupon_rate: &Bound<'_, PyAny>,
        lower_bound: &Bound<'_, PyAny>,
        upper_bound: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        realized_hit_count: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = core::RangeAccrualTerms::new(
            real(notional)?,
            real(annual_coupon_rate)?,
            real(lower_bound)?,
            real(upper_bound)?,
            date(effective_date)?,
            date(expiry_date)?,
        );
        if let Some(value) = realized_hit_count {
            terms.realized_hit_count = history(value)?;
        }
        Ok(Self(domain(py, core::RangeAccrualOption::new(terms))?))
    }

    #[pyo3(signature = (**fields))]
    fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(fields, &["realized_hit_count"])?;
        let count = match fields
            .map(|f| f.get_item("realized_hit_count"))
            .transpose()?
            .flatten()
        {
            Some(value) => history(&value)?,
            None => self.0.realized_hit_count(),
        };
        Ok(Self(domain(py, self.0.replace(count))?))
    }
    #[getter]
    fn notional(&self) -> f64 {
        self.0.notional()
    }
    #[getter]
    fn annual_coupon_rate(&self) -> f64 {
        self.0.annual_coupon_rate()
    }
    #[getter]
    fn lower_bound(&self) -> f64 {
        self.0.lower_bound()
    }
    #[getter]
    fn upper_bound(&self) -> f64 {
        self.0.upper_bound()
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
    #[getter]
    fn realized_hit_count(&self) -> Option<i32> {
        self.0.realized_hit_count()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "RangeAccrualOption(notional={:?}, annual_coupon_rate={:?}, lower_bound={:?}, upper_bound={:?}, effective_date={}, expiry_date={}, realized_hit_count={})",
            self.notional(),
            self.annual_coupon_rate(),
            self.lower_bound(),
            self.upper_bound(),
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            self.realized_hit_count()
                .map_or_else(|| "None".into(), |n| n.to_string())
        ))
    }
}
