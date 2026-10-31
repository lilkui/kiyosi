use crate::conversions::{integer, real};
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
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

fn scheme(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::FiniteDifferenceScheme> {
    match value.extract::<&str>()? {
        "explicit_euler" => Ok(kiyosi::FiniteDifferenceScheme::ExplicitEuler),
        "implicit_euler" => Ok(kiyosi::FiniteDifferenceScheme::ImplicitEuler),
        "crank_nicolson" => Ok(kiyosi::FiniteDifferenceScheme::CrankNicolson),
        _ => Err(PyValueError::new_err("unknown finite-difference scheme")),
    }
}

macro_rules! fd_engine {
    ($name:ident, $instrument:ty, |$py:ident, $option:ident, $engine:ident, $context:ident, $request:ident, $shifts:ident| $price:expr, $analysis:expr) => {
        /// Finite-difference pricing with immutable grid settings.
        ///
        /// Parameters
        /// ----------
        /// asset_step_count : int, optional
        ///     Number of spatial steps, at least 3; defaults to 800.
        /// time_step_count : int, optional
        ///     Positive time-step budget, default 200; event dates and curve knots
        ///     are added.
        /// scheme : FiniteDifferenceScheme, optional
        ///     Time-marching scheme; defaults to damped Crank-Nicolson.
        /// asset_upper_boundary : float or None, optional
        ///     Finite positive upper spot boundary, or None (default) for the
        ///     automatic domain. Must exceed spot, strike and barriers at pricing.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If asset_step_count is below 3, time_step_count is below 1, or an
        ///     explicit upper boundary is nonpositive or non-finite. Boundary coverage
        ///     and explicit-Euler stability are checked during pricing.
        ///
        /// Notes
        /// -----
        /// The engine name selects its product family. The vanilla engine accepts
        /// EuropeanOption and AmericanOption. The digital engine accepts
        /// CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
        /// also accepts TouchOption. The autocallable engine accepts all snowball
        /// variants and PhoenixOption. Other engines accept their named product.
        /// Digital pricing requires explicitly flat rates; other families accept
        /// deterministic rate curves.
        #[pyclass(frozen, module = "kiyosi._native")]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, asset_step_count=kiyosi::$name::default().settings().asset_step_count, time_step_count=kiyosi::$name::default().settings().time_step_count, scheme=kiyosi::$name::default().settings().scheme, asset_upper_boundary=None))]
            fn new(
                #[pyo3(from_py_with = integer)] asset_step_count: i32,
                #[pyo3(from_py_with = integer)] time_step_count: i32,
                #[pyo3(from_py_with = scheme)] scheme: kiyosi::FiniteDifferenceScheme,
                asset_upper_boundary: Option<&Bound<'_, PyAny>>,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::$name::new(kiyosi::FiniteDifferenceSettings {
                    asset_step_count,
                    time_step_count,
                    scheme,
                    asset_upper_boundary: asset_upper_boundary.map(real).transpose()?,
                }))?))
            }
            /// Return the configured spatial grid budget.
            #[getter]
            fn asset_step_count(&self) -> i32 {
                self.0.settings().asset_step_count
            }
            /// Return the configured time-step budget before event insertion.
            #[getter]
            fn time_step_count(&self) -> i32 {
                self.0.settings().time_step_count
            }
            /// Return the configured time-marching choice string.
            #[getter]
            fn scheme(&self) -> &'static str {
                match self.0.settings().scheme {
                    kiyosi::FiniteDifferenceScheme::ExplicitEuler => "explicit_euler",
                    kiyosi::FiniteDifferenceScheme::ImplicitEuler => "implicit_euler",
                    kiyosi::FiniteDifferenceScheme::CrankNicolson => "crank_nicolson",
                }
            }
            /// Return the explicit upper spot boundary, or None for automatic selection.
            #[getter]
            fn asset_upper_boundary(&self) -> Option<f64> {
                self.0.settings().asset_upper_boundary
            }

            /// Return the price without changing the instrument or context.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     Supported contract; see the engine class for accepted types.
            /// context : PricingContext
            ///     Market snapshot and valuation instant.
            ///
            /// Returns
            /// -------
            /// float
            ///     Present value of remaining contractual payments.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If domain validation, engine applicability or numerical pricing fails.
            fn price(
                &self,
                py: Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &PricingContext,
            ) -> PyResult<f64> {
                let $py = py;
                let $option = instrument;
                let $engine = self.0;
                let $context = &context.0;
                domain($price)
            }

            /// Return the price and selected sensitivities.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     Supported contract; see the engine class for accepted types.
            /// context : PricingContext
            ///     Market snapshot and valuation instant.
            /// greeks : str or iterable of str, optional
            ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
            /// all_greeks : bool, optional
            ///     Request every measure; omit `greeks` when True. Defaults to False.
            /// spot_shift : float, optional
            ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
            /// volatility_shift : float, optional
            ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
            /// rate_shift : float, optional
            ///     Positive finite parallel risk-free-rate bump in decimal units;
            ///     defaults to 0.0001.
            /// time_shift_days : int, optional
            ///     Positive forward/backward time bump in calendar days; defaults to 1.
            ///     All bumps are validated even when analytic Greeks are available.
            ///
            /// Returns
            /// -------
            /// PricingResult
            ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
            ///     including every Greek at expiry. Units follow PricingResult.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If requests, shifts, pricing or a feasible bumped valuation fails.
            #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false, spot_shift=kiyosi::NumericalShiftSettings::default().spot_shift, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
            #[expect(clippy::too_many_arguments, reason = "explicit Python pricing controls")]
            fn price_with_greeks(
                &self,
                py: Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &PricingContext,
                greeks: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
                #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
                #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
            ) -> PyResult<risk::PricingResult> {
                let $py = py;
                let $request = risk::greeks_request(greeks, all_greeks)?;
                let $shifts = kiyosi::NumericalShiftSettings { spot_shift, volatility_shift, rate_shift, time_shift_days };
                let $option = instrument;
                let $engine = self.0;
                let $context = &context.0;
                Ok(risk::PricingResult(domain($analysis)?))
            }
        }
    };
}

fd_engine!(
    FiniteDifferenceAccumulatorEngine,
    PyRef<'_, accumulator::Accumulator>,
    |py, option, engine, context, request, shifts| {
        let option = &option.0;
        py.detach(|| engine.price(option, context))
    },
    {
        let option = &option.0;
        py.detach(|| engine.price_with_greeks(option, context, request, shifts))
    }
);

fd_engine!(
    FiniteDifferenceVanillaEngine,
    binomial::VanillaInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        match option {
            binomial::VanillaInstrument::European(option) => {
                let option = &option.0;
                py.detach(|| engine.price(option, context))
            }
            binomial::VanillaInstrument::American(option) => {
                let option = &option.0;
                py.detach(|| engine.price(option, context))
            }
        }
    },
    {
        match option {
            binomial::VanillaInstrument::European(option) => {
                let option = &option.0;
                py.detach(|| engine.price_with_greeks(option, context, request, shifts))
            }
            binomial::VanillaInstrument::American(option) => {
                let option = &option.0;
                py.detach(|| engine.price_with_greeks(option, context, request, shifts))
            }
        }
    }
);
fd_engine!(
    FiniteDifferenceDigitalEngine,
    digital::DigitalInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        let option = option.contract();
        py.detach(|| engine.price(option, context))
    },
    {
        let option = option.contract();
        py.detach(|| engine.price_with_greeks(option, context, request, shifts))
    }
);

fd_engine!(
    FiniteDifferenceBarrierEngine,
    PyRef<'_, barrier::BarrierOption>,
    |py, option, engine, context, request, shifts| {
        let option = &option.0;
        py.detach(|| engine.price(option, context))
    },
    {
        let option = &option.0;
        py.detach(|| engine.price_with_greeks(option, context, request, shifts))
    }
);

fd_engine!(
    FiniteDifferenceBinaryBarrierEngine,
    binary_barrier::BinaryInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        match option {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = &option.0;
                py.detach(|| engine.price(option, context))
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = &option.0;
                py.detach(|| engine.price(option, context))
            }
        }
    },
    {
        match option {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = &option.0;
                py.detach(|| engine.price_with_greeks(option, context, request, shifts))
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = &option.0;
                py.detach(|| engine.price_with_greeks(option, context, request, shifts))
            }
        }
    }
);

fd_engine!(
    FiniteDifferenceDoubleBarrierEngine,
    PyRef<'_, double_barrier::DoubleBarrierOption>,
    |py, option, engine, context, request, shifts| {
        let option = &option.0;
        py.detach(|| engine.price(option, context))
    },
    {
        let option = &option.0;
        py.detach(|| engine.price_with_greeks(option, context, request, shifts))
    }
);

fd_engine!(
    FiniteDifferenceDoubleTouchEngine,
    PyRef<'_, double_touch::DoubleTouchOption>,
    |py, option, engine, context, request, shifts| {
        let option = &option.0;
        py.detach(|| engine.price(option, context))
    },
    {
        let option = &option.0;
        py.detach(|| engine.price_with_greeks(option, context, request, shifts))
    }
);

fd_engine!(
    FiniteDifferenceAutocallableEngine,
    Bound<'_, PyAny>,
    |py, option, engine, context, request, shifts| {
        autocallable::with_note!(&option, note, py.detach(|| engine.price(note, context)))
    },
    {
        autocallable::with_note!(
            &option,
            note,
            py.detach(|| engine.price_with_greeks(note, context, request, shifts))
        )
    }
);
