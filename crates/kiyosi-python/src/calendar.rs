use crate::conversions::{check_changes, date, dates, integer, python_date};
use crate::{core, error::domain};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

#[pyclass(frozen, eq, from_py_object, module = "kiyosi._native")]
#[derive(Clone, Default, PartialEq)]
pub(crate) struct TradingCalendar(pub(crate) core::TradingCalendar);

#[pymethods]
impl TradingCalendar {
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

    fn is_trading_day(&self, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0.is_trading_day(date(value)?))
    }

    fn adjust<'py>(
        &self,
        py: Python<'py>,
        nominal: &Bound<'_, PyAny>,
        convention: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyDate>> {
        let nominal = date(nominal)?;
        let convention = match convention.extract::<&str>()? {
            "following" => core::BusinessDayConvention::Following,
            "preceding" => core::BusinessDayConvention::Preceding,
            _ => {
                return Err(PyValueError::new_err(
                    "convention must be following or preceding",
                ));
            }
        };
        python_date(py, domain(py, self.0.adjust(nominal, convention))?)
    }

    fn trading_days_between(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<i32> {
        domain(py, self.0.trading_days_between(date(start)?, date(end)?))
    }

    fn trading_year_fraction(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(py, self.0.trading_year_fraction(date(start)?, date(end)?))
    }
}

#[pyfunction]
pub(crate) fn weekdays_calendar() -> TradingCalendar {
    TradingCalendar::default()
}

#[pyfunction]
pub(crate) fn all_days_calendar() -> TradingCalendar {
    TradingCalendar(core::TradingCalendar::all_days())
}

#[pyfunction]
pub(crate) fn sse_calendar() -> TradingCalendar {
    TradingCalendar(core::TradingCalendar::sse())
}

#[pyfunction]
#[pyo3(signature = (*, base, **overrides))]
pub(crate) fn custom_calendar(
    py: Python<'_>,
    base: &TradingCalendar,
    overrides: Option<&Bound<'_, PyDict>>,
) -> PyResult<TradingCalendar> {
    check_changes(
        overrides,
        &["closed_dates", "open_dates", "trading_days_per_year"],
    )?;
    let mut closed = Vec::new();
    let mut open = Vec::new();
    let mut basis = None;
    if let Some(overrides) = overrides {
        closed = overrides
            .get_item("closed_dates")?
            .as_ref()
            .map(dates)
            .transpose()?
            .unwrap_or_default();
        open = overrides
            .get_item("open_dates")?
            .as_ref()
            .map(dates)
            .transpose()?
            .unwrap_or_default();
        basis = overrides
            .get_item("trading_days_per_year")?
            .filter(|value| !value.is_none())
            .as_ref()
            .map(integer)
            .transpose()?;
    }
    Ok(TradingCalendar(domain(
        py,
        core::TradingCalendar::custom(base.0.clone(), closed, open, basis),
    )?))
}
