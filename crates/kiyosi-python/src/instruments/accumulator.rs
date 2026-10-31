use crate::conversions::{date, dates, direction, python_date, real};
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
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
/// accumulated_quantity : float, optional
///     Caller-asserted outstanding quantity strictly before valuation.
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
///     non-finite, expiry precedes effective_date, or explicit dates are invalid.
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
    #[new]
    #[pyo3(signature = (*, strike, knock_out_level, daily_quantity, acceleration_factor, effective_date, expiry_date, accumulated_quantity=kiyosi::AccumulatorTerms::DEFAULT_ACCUMULATED_QUANTITY, option_type=kiyosi::AccumulatorTerms::DEFAULT_OPTION_TYPE, knock_out_observation_dates=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        strike: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        daily_quantity: &Bound<'_, PyAny>,
        acceleration_factor: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] accumulated_quantity: f64,
        #[pyo3(from_py_with = direction)] option_type: kiyosi::OptionType,
        knock_out_observation_dates: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::AccumulatorTerms::new(
            real(strike)?,
            real(knock_out_level)?,
            real(daily_quantity)?,
            real(acceleration_factor)?,
            date(effective_date)?,
            date(expiry_date)?,
        );
        terms.accumulated_quantity = accumulated_quantity;
        terms.option_type = option_type;
        terms.knock_out_observation_dates = knock_out_observation_dates.map(dates).transpose()?;
        Ok(Self(domain(kiyosi::Accumulator::new(terms))?))
    }
    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// accumulated_quantity : float
    ///     Caller-asserted outstanding quantity strictly before valuation.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omitted fields retain their values.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
    #[pyo3(signature = (*, accumulated_quantity))]
    fn replace(&self, accumulated_quantity: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(self.0.replace(real(accumulated_quantity)?))?))
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
    fn accumulated_quantity(&self) -> f64 {
        self.0.accumulated_quantity()
    }
    /// Return inclusive first contract date.
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    /// Return final contract date; expiry is at midnight UTC.
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let observations = match self.knock_out_observation_dates(py)? {
            Some(dates) => PyList::new(py, dates)?.repr()?.to_string(),
            None => "None".into(),
        };
        Ok(format!(
            "Accumulator(strike={:?}, knock_out_level={:?}, daily_quantity={:?}, acceleration_factor={:?}, accumulated_quantity={:?}, effective_date={}, expiry_date={}, option_type={:?}, knock_out_observation_dates={})",
            self.strike(),
            self.knock_out_level(),
            self.daily_quantity(),
            self.acceleration_factor(),
            self.accumulated_quantity(),
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            self.option_type(),
            observations
        ))
    }
}
