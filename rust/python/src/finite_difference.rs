use super::*;

fn scheme(value: &Bound<'_, PyAny>) -> PyResult<core::FiniteDifferenceScheme> {
    match value.extract::<&str>()? {
        "explicit_euler" => Ok(core::FiniteDifferenceScheme::ExplicitEuler),
        "implicit_euler" => Ok(core::FiniteDifferenceScheme::ImplicitEuler),
        "crank_nicolson" => Ok(core::FiniteDifferenceScheme::CrankNicolson),
        _ => Err(PyValueError::new_err("unknown finite-difference scheme")),
    }
}

fn settings(fields: Option<&Bound<'_, PyDict>>) -> PyResult<core::FiniteDifferenceSettings> {
    check_changes(
        fields,
        &[
            "asset_step_count",
            "time_step_count",
            "scheme",
            "asset_upper_boundary",
        ],
    )?;
    let mut settings = core::FiniteDifferenceSettings::default();
    if let Some(fields) = fields {
        if let Some(value) = fields.get_item("asset_step_count")? {
            settings.asset_step_count = integer(&value)?;
        }
        if let Some(value) = fields.get_item("time_step_count")? {
            settings.time_step_count = integer(&value)?;
        }
        if let Some(value) = fields.get_item("scheme")? {
            settings.scheme = scheme(&value)?;
        }
        if let Some(value) = fields.get_item("asset_upper_boundary")? {
            settings.asset_upper_boundary = if value.is_none() {
                None
            } else {
                Some(real(&value)?)
            };
        }
    }
    Ok(settings)
}

macro_rules! fd_engine {
    ($name:ident, $instrument:ty, |$py:ident, $option:ident, $engine:ident, $context:ident, $request:ident, $shifts:ident| $price:expr, $analysis:expr) => {
        #[pyclass(frozen, module = "kiyosi._native")]
        pub(crate) struct $name(pub(crate) core::$name);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (**fields))]
            fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                Ok(Self(domain(py, core::$name::new(settings(fields)?))?))
            }
            #[getter]
            fn asset_step_count(&self) -> i32 {
                self.0.settings().asset_step_count
            }
            #[getter]
            fn time_step_count(&self) -> i32 {
                self.0.settings().time_step_count
            }
            #[getter]
            fn scheme(&self) -> &'static str {
                match self.0.settings().scheme {
                    core::FiniteDifferenceScheme::ExplicitEuler => "explicit_euler",
                    core::FiniteDifferenceScheme::ImplicitEuler => "implicit_euler",
                    core::FiniteDifferenceScheme::CrankNicolson => "crank_nicolson",
                }
            }
            #[getter]
            fn asset_upper_boundary(&self) -> Option<f64> {
                self.0.settings().asset_upper_boundary
            }

            fn price(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
            ) -> PyResult<f64> {
                let $py = py;
                let $option = instrument;
                let $engine = self.0;
                let $context = context.0.clone();
                domain(py, $price)
            }

            #[pyo3(signature = (instrument, context, greeks=None, **settings))]
            fn price_with_greeks(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                greeks: Option<&Bound<'_, PyAny>>,
                settings: Option<&Bound<'_, PyDict>>,
            ) -> PyResult<analytics::PricingResult> {
                let $py = py;
                let $request = analytics::request(greeks, settings)?;
                let $shifts = analytics::shifts(settings)?;
                let $option = instrument;
                let $engine = self.0;
                let $context = context.0.clone();
                Ok(analytics::PricingResult(domain(py, $analysis)?))
            }
        }
    };
}

fd_engine!(
    FiniteDifferenceVanillaEngine,
    binomial::VanillaInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        match option {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price(&option, &context))
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price(&option, &context))
            }
        }
    },
    {
        match option {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
            }
        }
    }
);
fd_engine!(
    FiniteDifferenceDigitalEngine,
    digital::DigitalInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        let option = option.contract().clone();
        py.detach(|| engine.price(&option, &context))
    },
    {
        let option = option.contract().clone();
        py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
    }
);

fd_engine!(
    FiniteDifferenceBarrierEngine,
    PyRef<'_, barrier::BarrierOption>,
    |py, option, engine, context, request, shifts| {
        let option = option.0.clone();
        py.detach(|| engine.price(&option, &context))
    },
    {
        let option = option.0.clone();
        py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
    }
);
