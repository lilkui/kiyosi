use super::*;

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct AmericanOption(pub(crate) core::AmericanOption);

#[pymethods]
impl AmericanOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date))]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let direction = direction(option_type)?;
        Ok(Self(domain(
            py,
            core::AmericanOption::new(
                direction,
                real(strike)?,
                date(effective_date)?,
                date(expiry_date)?,
            ),
        )?))
    }

    #[pyo3(signature = (**fields))]
    fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(
            fields,
            &["option_type", "strike", "effective_date", "expiry_date"],
        )?;
        let item = |name| {
            fields
                .map(|f| f.get_item(name))
                .transpose()
                .map(Option::flatten)
        };
        let changes = core::AmericanOptionChanges {
            option_type: item("option_type")?.as_ref().map(direction).transpose()?,
            strike: item("strike")?.as_ref().map(real).transpose()?,
            effective_date: item("effective_date")?.as_ref().map(date).transpose()?,
            expiry_date: item("expiry_date")?.as_ref().map(date).transpose()?,
        };
        Ok(Self(domain(py, self.0.replace(changes))?))
    }

    #[getter]
    fn option_type(&self) -> &'static str {
        match self.0.option_type() {
            core::OptionType::Call => "call",
            core::OptionType::Put => "put",
        }
    }
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct BjerksundStenslandVanillaEngine;

#[pymethods]
impl BjerksundStenslandVanillaEngine {
    #[new]
    fn new() -> Self {
        Self
    }

    fn price(
        &self,
        py: Python<'_>,
        instrument: &AmericanOption,
        context: &PricingContext,
    ) -> PyResult<f64> {
        domain(
            py,
            core::BjerksundStenslandVanillaEngine.price(&instrument.0, &context.0),
        )
    }

    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &AmericanOption,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        Ok(analytics::PricingResult(domain(
            py,
            core::BjerksundStenslandVanillaEngine.price_with_greeks(
                &instrument.0,
                &context.0,
                request,
                shifts,
            ),
        )?))
    }
}
