use crate::conversions::{date, dates, integer, python_date};
use crate::error::domain;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDate;

/// Immutable trading calendar with a fixed annualization basis.
///
/// Notes
/// -----
/// Use the calendar factory functions to construct instances.
#[pyclass(frozen, eq, from_py_object, module = "kiyosi._native")]
#[derive(Clone, Default, PartialEq)]
pub(crate) struct TradingCalendar(pub(crate) kiyosi::TradingCalendar);

#[pymethods]
impl TradingCalendar {
    /// Return the positive calendar annualization basis.
    #[getter]
    fn trading_days_per_year(&self) -> i32 {
        self.0.trading_days_per_year()
    }

    fn __repr__(&self) -> String {
        format!(
            "TradingCalendar(trading_days_per_year={})",
            self.0.trading_days_per_year()
        )
    }

    /// Return whether a date is open in this calendar.
    ///
    /// Parameters
    /// ----------
    /// value : datetime.date
    ///     Date to test; datetime values are not accepted.
    ///
    /// Returns
    /// -------
    /// bool
    ///     Whether the date is a trading day, including explicit overrides.
    fn is_trading_day(&self, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0.is_trading_day(date(value)?))
    }

    /// Find a trading date in the selected direction, including the input date.
    ///
    /// Parameters
    /// ----------
    /// nominal : datetime.date
    ///     Unadjusted date.
    /// convention : {'following', 'preceding'}
    ///     Direction to search.
    ///
    /// Returns
    /// -------
    /// datetime.date
    ///     First trading date encountered.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the supported date range is exhausted.
    fn adjust<'py>(
        &self,
        py: Python<'py>,
        nominal: &Bound<'_, PyAny>,
        convention: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyDate>> {
        let nominal = date(nominal)?;
        let convention = match convention.extract::<&str>()? {
            "following" => kiyosi::BusinessDayConvention::Following,
            "preceding" => kiyosi::BusinessDayConvention::Preceding,
            _ => {
                return Err(PyValueError::new_err(
                    "convention must be following or preceding",
                ));
            }
        };
        python_date(py, domain(self.0.adjust(nominal, convention))?)
    }

    /// Count trading dates in the half-open interval [start, end).
    ///
    /// Parameters
    /// ----------
    /// start, end : datetime.date
    ///     Ordered interval endpoints.
    ///
    /// Returns
    /// -------
    /// int
    ///     Number of trading dates.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If end precedes start.
    fn trading_days_between(
        &self,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<i32> {
        domain(self.0.trading_days_between(date(start)?, date(end)?))
    }

    /// Return trading-day count divided by the calendar annualization basis.
    ///
    /// Parameters
    /// ----------
    /// start, end : datetime.date
    ///     Ordered endpoints of the half-open interval [start, end).
    ///
    /// Returns
    /// -------
    /// float
    ///     Trading-year fraction.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If end precedes start.
    fn trading_year_fraction(
        &self,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(self.0.trading_year_fraction(date(start)?, date(end)?))
    }
}

/// Create a Monday-through-Friday calendar with a 252-day basis.
///
/// Returns
/// -------
/// TradingCalendar
///     Calendar with the stated annualization basis.
#[pyfunction]
pub(crate) fn weekdays_calendar() -> TradingCalendar {
    TradingCalendar::default()
}

/// Create a calendar with every date open and a 365-day basis.
///
/// Returns
/// -------
/// TradingCalendar
///     Calendar with the stated annualization basis.
#[pyfunction]
pub(crate) fn all_days_calendar() -> TradingCalendar {
    TradingCalendar(kiyosi::TradingCalendar::all_days())
}

/// Create an SSE weekday-closure calendar with a 243-day basis.
///
/// Returns
/// -------
/// TradingCalendar
///     Calendar with the stated annualization basis.
///
/// Notes
/// -----
/// Uses recorded and forecast weekday closures for 1991-2099, and weekdays
/// outside that range. Forecasts omit ad-hoc closures.
#[pyfunction]
pub(crate) fn sse_calendar() -> TradingCalendar {
    TradingCalendar(kiyosi::TradingCalendar::sse())
}

/// Create open and closed overrides without changing the base calendar.
///
/// Parameters
/// ----------
/// base : TradingCalendar
///     Base calendar; the original is unchanged.
/// closed_dates : iterable of date, optional
///     Dates forced closed; duplicates and unsorted input are accepted.
/// open_dates : iterable of date, optional
///     Dates forced open, including weekends; must not also be closed.
/// trading_days_per_year : int or None, optional
///     Positive annualization basis, or None to retain the base basis.
///
/// Returns
/// -------
/// TradingCalendar
///     Owned result; no input is mutated.
///
/// Raises
/// ------
/// InvalidInputError
///     If a date is both open and closed, or the annualization basis is nonpositive.
#[pyfunction]
#[pyo3(signature = (*, base, closed_dates=Vec::new(), open_dates=Vec::new(), trading_days_per_year=None))]
pub(crate) fn custom_calendar(
    base: &TradingCalendar,
    #[pyo3(from_py_with = dates)] closed_dates: Vec<kiyosi::Date>,
    #[pyo3(from_py_with = dates)] open_dates: Vec<kiyosi::Date>,
    trading_days_per_year: Option<&Bound<'_, PyAny>>,
) -> PyResult<TradingCalendar> {
    let basis = trading_days_per_year.map(integer).transpose()?;
    Ok(TradingCalendar(domain(kiyosi::TradingCalendar::custom(
        base.0.clone(),
        closed_dates,
        open_dates,
        basis,
    ))?))
}
