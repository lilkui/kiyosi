use super::*;

fn changes(fields: Option<&Bound<'_, PyDict>>, cash: bool) -> PyResult<core::DigitalOptionChanges> {
    let allowed = if cash {
        &[
            "option_type",
            "strike",
            "payout",
            "effective_date",
            "expiry_date",
        ][..]
    } else {
        &["option_type", "strike", "effective_date", "expiry_date"][..]
    };
    check_changes(fields, allowed)?;
    let item = |name| {
        fields
            .map(|f| f.get_item(name))
            .transpose()
            .map(Option::flatten)
    };
    Ok(core::DigitalOptionChanges {
        option_type: item("option_type")?.as_ref().map(direction).transpose()?,
        strike: item("strike")?.as_ref().map(real).transpose()?,
        payout: item("payout")?.as_ref().map(real).transpose()?,
        effective_date: item("effective_date")?.as_ref().map(date).transpose()?,
        expiry_date: item("expiry_date")?.as_ref().map(date).transpose()?,
    })
}

macro_rules! digital_option {
    ($name:ident, $constructor:ident $(, $payout:ident)?) => {
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, PartialEq)]
        pub(crate) struct $name(pub core::DigitalOption);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, option_type, strike, $($payout,)? effective_date, expiry_date))]
            fn new(py: Python<'_>, option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                $($payout: &Bound<'_, PyAny>,)? effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                Ok(Self(domain(py, core::DigitalOption::$constructor(direction(option_type)?,
                    real(strike)?, $(real($payout)?,)? date(effective_date)?, date(expiry_date)?) )?))
            }

            #[pyo3(signature = (**fields))]
            fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                let changes = changes(fields, self.0.payout().is_some())?;
                Ok(Self(domain(py, self.0.replace(changes))?))
            }

            #[getter]
            fn option_type(&self) -> &'static str {
                if self.0.option_type() == core::OptionType::Call { "call" } else { "put" }
            }
            #[getter]
            fn strike(&self) -> f64 { self.0.strike() }
            #[getter]
            fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.effective_date())
            }
            #[getter]
            fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.expiry_date())
            }
            $(
                #[getter]
                fn $payout(&self) -> Option<f64> { self.0.payout() }
            )?
        }
    };
}

digital_option!(CashOrNothingOption, cash_or_nothing, payout);
digital_option!(AssetOrNothingOption, asset_or_nothing);

#[derive(FromPyObject)]
pub(crate) enum DigitalInstrument<'py> {
    Cash(PyRef<'py, CashOrNothingOption>),
    Asset(PyRef<'py, AssetOrNothingOption>),
}
impl DigitalInstrument<'_> {
    pub(crate) fn contract(&self) -> &core::DigitalOption {
        match self {
            Self::Cash(option) => &option.0,
            Self::Asset(option) => &option.0,
        }
    }
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticDigitalEngine;

#[pymethods]
impl AnalyticDigitalEngine {
    #[new]
    fn new() -> Self {
        Self
    }

    fn price(
        &self,
        py: Python<'_>,
        instrument: DigitalInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        domain(
            py,
            core::AnalyticDigitalEngine.price(instrument.contract(), &context.0),
        )
    }

    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: DigitalInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        Ok(analytics::PricingResult(domain(
            py,
            core::AnalyticDigitalEngine.price_with_greeks(
                instrument.contract(),
                &context.0,
                request,
                shifts,
            ),
        )?))
    }
}
