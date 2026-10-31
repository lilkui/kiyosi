use crate::conversions::integer;
use crate::error::domain;
use crate::instruments::american;
use crate::instruments::barrier::BarrierOption;
use crate::instruments::digital;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;

#[derive(FromPyObject)]
pub(crate) enum VanillaInstrument<'py> {
    European(PyRef<'py, EuropeanOption>),
    American(PyRef<'py, american::AmericanOption>),
}

macro_rules! with_vanilla {
    ($instrument:expr, $option:ident, $body:expr) => {{
        match $instrument {
            $crate::engines::binomial::VanillaInstrument::European(option) => {
                let $option = &option.0;
                $body
            }
            $crate::engines::binomial::VanillaInstrument::American(option) => {
                let $option = &option.0;
                $body
            }
        }
    }};
}
pub(crate) use with_vanilla;

macro_rules! binomial_engine {
    ($name:ident, $instrument:ty, |$py:ident, $option:ident, $engine:ident, $context:ident, $request:ident, $shifts:ident| $price:expr, $analysis:expr) => {
        /// Flat-rate Cox-Ross-Rubinstein tree pricing.
        ///
        /// Parameters
        /// ----------
        /// step_count : int, optional
        ///     Number of tree time steps, from 1 through 1,000,000; defaults to 256.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If step_count is outside 1 through 1,000,000.
        ///
        /// Notes
        /// -----
        /// The vanilla engine accepts EuropeanOption and AmericanOption; the
        /// digital engine accepts CashOrNothingOption and AssetOrNothingOption.
        #[pyclass(frozen, module = "kiyosi._native")]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, step_count=kiyosi::BinomialSettings::default().step_count))]
            fn new(
                #[pyo3(from_py_with = integer)] step_count: i32,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::$name::new(kiyosi::BinomialSettings { step_count }))?))
            }

            /// Return the configured grid size.
            #[getter]
            fn step_count(&self) -> i32 {
                self.0.settings().step_count
            }

            fn __repr__(&self) -> String {
                format!("{}(step_count={})", stringify!($name), self.step_count())
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
                let $option = instrument;
                let $engine = self.0;
                let $context = &context.0;
                let $request = risk::greeks_request(greeks, all_greeks)?;
                let $shifts = kiyosi::NumericalShiftSettings { spot_shift, volatility_shift, rate_shift, time_shift_days };
                Ok(risk::PricingResult(domain($analysis)?))
            }
        }
    };
}

binomial_engine!(
    CoxRossRubinsteinVanillaEngine,
    VanillaInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        with_vanilla!(option, option, py.detach(|| engine.price(option, context)))
    },
    {
        with_vanilla!(
            option,
            option,
            py.detach(|| engine.price_with_greeks(option, context, request, shifts))
        )
    }
);
binomial_engine!(
    CoxRossRubinsteinDigitalEngine,
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
/// Flat-rate Cox-Ross-Rubinstein tree pricing.
///
/// Parameters
/// ----------
/// step_count : int, optional
///     Number of tree time steps, from 1 through 1,000,000; defaults to 256.
///
/// Raises
/// ------
/// InvalidInputError
///     If step_count is outside 1 through 1,000,000.
///
/// See Also
/// --------
/// BarrierOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct CoxRossRubinsteinBarrierEngine(pub(crate) kiyosi::CoxRossRubinsteinBarrierEngine);

#[pymethods]
impl CoxRossRubinsteinBarrierEngine {
    #[new]
    #[pyo3(signature = (*, step_count=kiyosi::BinomialSettings::default().step_count))]
    fn new(#[pyo3(from_py_with = integer)] step_count: i32) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::CoxRossRubinsteinBarrierEngine::new(
            kiyosi::BinomialSettings { step_count },
        ))?))
    }
    /// Return the configured grid size.
    #[getter]
    fn step_count(&self) -> i32 {
        self.0.settings().step_count
    }
    fn __repr__(&self) -> String {
        format!(
            "CoxRossRubinsteinBarrierEngine(step_count={})",
            self.step_count()
        )
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
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, BarrierOption>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = &instrument.0;
        let context = &context.0;
        domain(py.detach(|| engine.price(option, context)))
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
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python pricing controls"
    )]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, BarrierOption>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
        #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
        #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::greeks_request(greeks, all_greeks)?;
        let shifts = kiyosi::NumericalShiftSettings {
            spot_shift,
            volatility_shift,
            rate_shift,
            time_shift_days,
        };
        let engine = self.0;
        let option = &instrument.0;
        let context = &context.0;
        Ok(risk::PricingResult(domain(py.detach(|| {
            engine.price_with_greeks(option, context, request, shifts)
        }))?))
    }
}
