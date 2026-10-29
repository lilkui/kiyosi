use crate::conversions::{check_changes, integer, real};
use crate::engines::binomial;
use crate::error::domain;
use crate::instruments::accumulator;
use crate::instruments::autocallable;
use crate::instruments::barrier;
use crate::instruments::binary_barrier;
use crate::instruments::digital;
use crate::instruments::double_barrier;
use crate::instruments::double_touch;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

fn scheme(value: &Bound<'_, PyAny>) -> PyResult<core::FiniteDifferenceScheme> {
    match value.extract::<&str>()? {
        "explicit_euler" => Ok(core::FiniteDifferenceScheme::ExplicitEuler),
        "implicit_euler" => Ok(core::FiniteDifferenceScheme::ImplicitEuler),
        "crank_nicolson" => Ok(core::FiniteDifferenceScheme::CrankNicolson),
        _ => Err(PyValueError::new_err("unknown finite-difference scheme")),
    }
}

fn settings(
    fields: Option<&Bound<'_, PyDict>>,
    mut settings: core::FiniteDifferenceSettings,
) -> PyResult<core::FiniteDifferenceSettings> {
    check_changes(
        fields,
        &[
            "asset_step_count",
            "time_step_count",
            "scheme",
            "asset_upper_boundary",
        ],
    )?;
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
                Ok(Self(domain(
                    py,
                    core::$name::new(settings(fields, core::$name::default().settings())?),
                )?))
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
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
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
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &PricingContext,
                greeks: Option<&Bound<'_, PyAny>>,
                settings: Option<&Bound<'_, PyDict>>,
            ) -> PyResult<risk::PricingResult> {
                let $py = py;
                let $request = risk::request(greeks, settings)?;
                let $shifts = risk::shifts(settings)?;
                let $option = instrument;
                let $engine = self.0;
                let $context = context.0.clone();
                Ok(risk::PricingResult(domain(py, $analysis)?))
            }
        }
    };
}

fd_engine!(
    FiniteDifferenceAccumulatorEngine,
    PyRef<'_, accumulator::Accumulator>,
    |py, option, engine, context, request, shifts| {
        let option = option.0.clone();
        py.detach(|| engine.price(&option, &context))
    },
    {
        let option = option.0.clone();
        py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
    }
);

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

fd_engine!(
    FiniteDifferenceBinaryBarrierEngine,
    binary_barrier::BinaryInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        match option {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price(&option, &context))
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price(&option, &context))
            }
        }
    },
    {
        match option {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
            }
        }
    }
);

fd_engine!(
    FiniteDifferenceDoubleBarrierEngine,
    PyRef<'_, double_barrier::DoubleBarrierOption>,
    |py, option, engine, context, request, shifts| {
        let option = option.0.clone();
        py.detach(|| engine.price(&option, &context))
    },
    {
        let option = option.0.clone();
        py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
    }
);

fd_engine!(
    FiniteDifferenceDoubleTouchEngine,
    PyRef<'_, double_touch::DoubleTouchOption>,
    |py, option, engine, context, request, shifts| {
        let option = option.0.clone();
        py.detach(|| engine.price(&option, &context))
    },
    {
        let option = option.0.clone();
        py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
    }
);

fd_engine!(
    FiniteDifferenceAutocallableEngine,
    Bound<'_, PyAny>,
    |py, option, engine, context, request, shifts| {
        autocallable::with_note!(&option, note, py.detach(|| engine.price(&note, &context)))
    },
    {
        autocallable::with_note!(
            &option,
            note,
            py.detach(|| engine.price_with_greeks(&note, &context, request, shifts))
        )
    }
);
