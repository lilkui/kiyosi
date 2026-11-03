use crate::conversions::{date, dates, direction, python_date, real, supplied};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyList};

/// Accumulates purchases or sales daily, settling at scheduled knock-out or expiry.
///
/// Parameters
/// ----------
/// strike : float
///     Positive finite purchase or sale strike.
/// knock_out_level : float
///     Positive inclusive knock-out threshold.
/// daily_quantity : float
///     Nonnegative finite quantity bought or sold on each trading date.
/// acceleration_factor : float
///     Nonnegative multiplier below strike for calls or above strike for puts.
/// effective_midnight_utc : datetime.date
///     Contract starts at 00:00 UTC on this date.
/// expiry_midnight_utc : datetime.date
///     Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.
/// accumulated_quantity : float or None, optional
///     Outstanding quantity strictly before valuation. None means unknown; zero means no units.
///     Explicit history is required after any prior trading-date observation.
/// option_type : {'call', 'put'}, optional
///     Call accumulates purchases (the default); put accumulates sales.
/// knock_out_observation_dates : iterable of date or None, optional
///     Strictly increasing, unique dates within the inclusive contract life.
///     None observes knock-out every trading date; an empty sequence disables
///     early knock-out. Dates need not include either contract endpoint.
///
/// Raises
/// ------
/// InvalidInputError
///     If strike or knock-out level is invalid, quantities are negative or
///     non-finite, expiry precedes effective_midnight_utc, or explicit dates are invalid.
///     Trading-date compatibility is checked against the calendar at pricing.
///
/// Notes
/// -----
/// Events occur at midnight UTC on trading dates. On a knock-out observation
/// date, calls knock out at or above the barrier and puts at or below it.
/// Knock-out precedes accumulation and adds no units. Other trading dates
/// accumulate even beyond the barrier. At strike, the base quantity applies.
/// Expiry settles after its event, whether or not it is a knock-out date.
/// Calls pay quantity * (settlement spot - strike); puts pay its negative.
/// Intraday history includes that day's earlier accumulation. Do not value a
/// contract whose settlement has already occurred.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct Accumulator(pub(crate) kiyosi::Accumulator);

#[pymethods]
impl Accumulator {
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// knock_out_level : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// daily_quantity : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// acceleration_factor : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// accumulated_quantity : float or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_midnight_utc : datetime.date, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_midnight_utc : datetime.date, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// knock_out_observation_dates : Iterable[date] | None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    ///
    /// Returns
    /// -------
    /// object
    ///     Independent contract of the same class, preserving omitted history and terms.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation; the original is unchanged.
    /// TypeError
    ///     If a representation, field name or mutually exclusive coupon choice is invalid.
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, knock_out_level=crate::conversions::OMITTED, daily_quantity=crate::conversions::OMITTED, acceleration_factor=crate::conversions::OMITTED, accumulated_quantity=crate::conversions::OMITTED, effective_midnight_utc=crate::conversions::OMITTED, expiry_midnight_utc=crate::conversions::OMITTED, knock_out_observation_dates=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_out_level: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] daily_quantity: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] acceleration_factor: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] accumulated_quantity: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_midnight_utc: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_midnight_utc: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_out_observation_dates: Option<
            &Bound<'_, PyAny>,
        >,
    ) -> PyResult<Self> {
        let changes = kiyosi::AccumulatorChanges {
            option_type: option_type.map(crate::conversions::direction).transpose()?,
            strike: strike.map(crate::conversions::real).transpose()?,
            knock_out_level: knock_out_level.map(crate::conversions::real).transpose()?,
            daily_quantity: daily_quantity.map(crate::conversions::real).transpose()?,
            acceleration_factor: acceleration_factor
                .map(crate::conversions::real)
                .transpose()?,
            accumulated_quantity: accumulated_quantity
                .map(|value| {
                    if value.is_none() {
                        Ok(None)
                    } else {
                        crate::conversions::real(value).map(Some)
                    }
                })
                .transpose()?,
            effective_midnight_utc: effective_midnight_utc
                .map(crate::conversions::date)
                .transpose()?,
            expiry_midnight_utc: expiry_midnight_utc
                .map(crate::conversions::date)
                .transpose()?,
            knock_out_observation_dates: knock_out_observation_dates
                .map(|value| {
                    if value.is_none() {
                        Ok(None)
                    } else {
                        crate::conversions::dates(value).map(Some)
                    }
                })
                .transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
    #[new]
    #[pyo3(signature = (*, strike, knock_out_level, daily_quantity, acceleration_factor, effective_midnight_utc, expiry_midnight_utc, accumulated_quantity=None, option_type=kiyosi::AccumulatorTerms::DEFAULT_OPTION_TYPE, knock_out_observation_dates=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        strike: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        daily_quantity: &Bound<'_, PyAny>,
        acceleration_factor: &Bound<'_, PyAny>,
        effective_midnight_utc: &Bound<'_, PyAny>,
        expiry_midnight_utc: &Bound<'_, PyAny>,
        accumulated_quantity: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = direction)] option_type: kiyosi::OptionType,
        knock_out_observation_dates: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::AccumulatorTerms::new(
            real(strike)?,
            real(knock_out_level)?,
            real(daily_quantity)?,
            real(acceleration_factor)?,
            date(effective_midnight_utc)?,
            date(expiry_midnight_utc)?,
        );
        terms.accumulated_quantity = accumulated_quantity.map(real).transpose()?;
        terms.option_type = option_type;
        terms.knock_out_observation_dates = knock_out_observation_dates.map(dates).transpose()?;
        Ok(Self(domain(kiyosi::Accumulator::new(terms))?))
    }
    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// accumulated_quantity : float or None, optional
    ///     Outstanding quantity strictly before valuation. None means unknown; zero means no units.
    ///     Explicit history is required after any prior trading-date observation.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omission or Ellipsis retains history.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
    #[pyo3(signature = (*, accumulated_quantity=crate::conversions::OMITTED))]
    fn replace_history(
        &self,
        #[pyo3(from_py_with = supplied)] accumulated_quantity: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let quantity = match accumulated_quantity {
            Some(value) if value.is_none() => None,
            Some(value) => Some(real(value)?),
            None => self.0.accumulated_quantity(),
        };
        Ok(Self(domain(self.0.replace_history(quantity))?))
    }
    /// Returns call for purchases or put for sales.
    #[getter]
    fn option_type(&self) -> &'static str {
        match self.0.option_type() {
            kiyosi::OptionType::Call => "call",
            kiyosi::OptionType::Put => "put",
        }
    }
    /// Returns owned knock-out dates, or None for every trading date.
    #[getter]
    fn knock_out_observation_dates<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Option<Vec<Bound<'py, PyDate>>>> {
        self.0
            .knock_out_observation_dates()
            .map(|dates| dates.iter().map(|&date| python_date(py, date)).collect())
            .transpose()
    }
    /// Returns the purchase or sale strike in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    /// Return positive inclusive knock-out threshold.
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level()
    }
    /// Returns the nonnegative base quantity bought or sold on each trading date.
    #[getter]
    fn daily_quantity(&self) -> f64 {
        self.0.daily_quantity()
    }
    /// Returns the quantity multiplier below strike for calls or above it for puts.
    #[getter]
    fn acceleration_factor(&self) -> f64 {
        self.0.acceleration_factor()
    }
    /// Return caller-asserted outstanding quantity strictly before valuation.
    #[getter]
    fn accumulated_quantity(&self) -> Option<f64> {
        self.0.accumulated_quantity()
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
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let observations = match self.knock_out_observation_dates(py)? {
            Some(dates) => PyList::new(py, dates)?.repr()?.to_string(),
            None => "None".into(),
        };
        Ok(format!(
            "Accumulator(strike={:?}, knock_out_level={:?}, daily_quantity={:?}, acceleration_factor={:?}, accumulated_quantity={}, effective_midnight_utc={}, expiry_midnight_utc={}, option_type={:?}, knock_out_observation_dates={})",
            self.strike(),
            self.knock_out_level(),
            self.daily_quantity(),
            self.acceleration_factor(),
            self.accumulated_quantity()
                .map_or_else(|| "None".into(), |value| format!("{value:?}")),
            self.effective_midnight_utc(py)?.repr()?,
            self.expiry_midnight_utc(py)?.repr()?,
            self.option_type(),
            observations
        ))
    }
}
