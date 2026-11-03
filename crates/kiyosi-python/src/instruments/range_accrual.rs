use crate::conversions::{date, integer, python_date, real, supplied};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::PyDate;

fn history(value: &Bound<'_, PyAny>) -> PyResult<Option<i32>> {
    if value.is_none() {
        Ok(None)
    } else {
        integer(value).map(Some)
    }
}

/// Range coupon paid at expiry without principal repayment.
///
/// Parameters
/// ----------
/// notional : float
///     Positive finite coupon notional, without principal repayment.
/// annual_coupon_rate : float
///     Nonnegative finite annual coupon rate in decimal units.
/// lower_bound : float
///     Inclusive nonnegative lower spot boundary.
/// upper_bound : float
///     Inclusive finite upper spot boundary, above the lower boundary.
/// effective_midnight_utc : datetime.date
///     Contract starts at 00:00 UTC on this date.
/// expiry_midnight_utc : datetime.date
///     Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.
/// realized_hit_count : int or None, optional
///     Hits strictly before valuation, or None for unknown history.
///
/// Raises
/// ------
/// InvalidInputError
///     If amounts, bounds or history are invalid, or expiry is not after
///     effective_midnight_utc. Pricing checks history against elapsed observations.
///
/// Notes
/// -----
/// Observe trading dates in (effective_midnight_utc, expiry_midnight_utc]. Bounds are
/// inclusive. Payment is notional times annual coupon times Actual/365 Fixed
/// life times the fraction of observations inside the range.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct RangeAccrualOption(pub kiyosi::RangeAccrualOption);

#[pymethods]
impl RangeAccrualOption {
    #[new]
    #[pyo3(signature = (*, notional, annual_coupon_rate, lower_bound, upper_bound, effective_midnight_utc, expiry_midnight_utc, realized_hit_count=None))]
    fn new(
        notional: &Bound<'_, PyAny>,
        annual_coupon_rate: &Bound<'_, PyAny>,
        lower_bound: &Bound<'_, PyAny>,
        upper_bound: &Bound<'_, PyAny>,
        effective_midnight_utc: &Bound<'_, PyAny>,
        expiry_midnight_utc: &Bound<'_, PyAny>,
        realized_hit_count: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::RangeAccrualTerms::new(
            real(notional)?,
            real(annual_coupon_rate)?,
            real(lower_bound)?,
            real(upper_bound)?,
            date(effective_midnight_utc)?,
            date(expiry_midnight_utc)?,
        );
        if let Some(value) = realized_hit_count {
            terms.realized_hit_count = history(value)?;
        }
        Ok(Self(domain(kiyosi::RangeAccrualOption::new(terms))?))
    }

    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// realized_hit_count : int or None, optional
    ///     Hits strictly before valuation, or None for unknown history.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omitted fields or Ellipsis retain their values.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
    #[pyo3(signature = (*, realized_hit_count=crate::conversions::OMITTED))]
    fn replace_history(
        &self,
        #[pyo3(from_py_with = supplied)] realized_hit_count: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let count = match realized_hit_count {
            Some(value) => history(value)?,
            None => self.0.realized_hit_count(),
        };
        Ok(Self(domain(self.0.replace_history(count))?))
    }
    /// Return positive finite coupon notional, without principal repayment.
    #[getter]
    fn notional(&self) -> f64 {
        self.0.notional()
    }
    /// Return nonnegative finite annual coupon rate in decimal units.
    #[getter]
    fn annual_coupon_rate(&self) -> f64 {
        self.0.annual_coupon_rate()
    }
    /// Return inclusive nonnegative lower spot boundary.
    #[getter]
    fn lower_bound(&self) -> f64 {
        self.0.lower_bound()
    }
    /// Return inclusive finite upper spot boundary, above the lower boundary.
    #[getter]
    fn upper_bound(&self) -> f64 {
        self.0.upper_bound()
    }
    /// Return inclusive first contract date.
    #[getter]
    fn effective_midnight_utc<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_midnight_utc())
    }
    /// Return final contract date; expiry is at midnight UTC.
    #[getter]
    fn expiry_midnight_utc<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_midnight_utc())
    }
    /// Return hits strictly before valuation, or None for unknown history.
    #[getter]
    fn realized_hit_count(&self) -> Option<i32> {
        self.0.realized_hit_count()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "RangeAccrualOption(notional={:?}, annual_coupon_rate={:?}, lower_bound={:?}, upper_bound={:?}, effective_midnight_utc={}, expiry_midnight_utc={}, realized_hit_count={})",
            self.notional(),
            self.annual_coupon_rate(),
            self.lower_bound(),
            self.upper_bound(),
            self.effective_midnight_utc(py)?.repr()?,
            self.expiry_midnight_utc(py)?.repr()?,
            self.realized_hit_count()
                .map_or_else(|| "None".into(), |n| n.to_string())
        ))
    }
}
