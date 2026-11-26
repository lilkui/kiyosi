use crate::conversions::integer;
use crate::error::domain;
use crate::instruments::american;
use crate::instruments::barrier::BarrierOption;
use crate::instruments::digital;
use crate::instruments::vanilla::EuropeanOption;
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
    ($(#[$doc:meta])* $name:ident, $instrument:ty, $with:path) => {
        binomial_engine!(
            $(#[$doc])* $name, $instrument, $with,
            [
                ///
                /// Notes
                /// -----
                /// At positive volatility before expiry delta and gamma use tree nodes. Zero
                /// volatility uses scalar differences. Other Greeks are unsupported. At expiry
                /// spot Greeks use terminal-payoff stencils with the request's shifts.
                /// Use numerical_greeks for scalar differences throughout.
                ///
            ]
        );
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty, $with:path,
     [$(#[$greeks_doc:meta])*] $(, {$($extra:tt)*})?) => {
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
        ///     Pricing paths that roll back every tree level also reject counts above 16,000.
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        engine_methods!(@price $name, $instrument,
            [$(#[$greeks_doc])*],
            [],
            {
                #[classattr]
                const _STATE_FIELDS: &'static [&'static str] = &["step_count"];

                #[new]
                #[pyo3(signature = (*, step_count=kiyosi::BinomialSettings::default().step_count))]
                fn new(
                    #[pyo3(from_py_with = integer)] step_count: i32,
                ) -> PyResult<Self> {
                    Ok(Self(domain(kiyosi::$name::new(kiyosi::BinomialSettings { step_count }))?))
                }

                /// Return the configured number of binomial time steps.
                #[getter]
                fn step_count(&self) -> i32 {
                    self.0.settings().step_count
                }
                $($($extra)*)?
            }
        );
        engine_methods!(@evaluate $name, $instrument, $with);
    };
}

binomial_engine!(
    ///
    /// Notes
    /// -----
    /// Accepts EuropeanOption and AmericanOption.
    ///
    CoxRossRubinsteinVanillaEngine,
    VanillaInstrument<'_>,
    with_vanilla,
    [
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use tree nodes;
        /// one-step gamma and other Greeks use numerical differences with the
        /// request's shifts. Zero volatility also uses scalar differences. At expiry
        /// spot Greeks use terminal-payoff stencils with the request's shifts.
        /// Use numerical_greeks for scalar differences throughout.
        ///
    ],
    {
        /// Price an American put with a bound on the discarded lattice tails.
        ///
        /// Parameters
        /// ----------
        /// instrument : AmericanOption
        ///     American put to price.
        /// context : PricingContext
        ///     Market data with flat rates.
        /// absolute_tolerance : float
        ///     Positive truncation tolerance in price units. Grid discretization
        ///     and floating-point errors still apply.
        ///
        /// Returns
        /// -------
        /// float
        ///     Price calculated on the bounded tree, or on the full tree when cheaper.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If the tolerance or valuation is invalid, or rollback work exceeds the limit.
        /// UnsupportedOperationError
        ///     If the American option is a call or the rate curves are nonconstant.
        #[expect(clippy::needless_pass_by_value, reason = "PyO3 owns the extracted instrument.")]
        #[pyo3(signature = (instrument, context, *, absolute_tolerance))]
        fn price_with_rollback_tolerance(
            &self,
            py: Python<'_>,
            #[pyo3(from_py_with = crate::conversions::instrument)]
            instrument: PyRef<'_, american::AmericanOption>,
            context: &crate::market::PricingContext,
            #[pyo3(from_py_with = crate::conversions::real)] absolute_tolerance: f64,
        ) -> PyResult<f64> {
            let instrument = &instrument.0;
            let context = &context.0;
            domain(crate::cancellation::run(py, || {
                self.0.price_with_rollback_tolerance(instrument, context, absolute_tolerance)
            })?)
        }
    }
);

binomial_engine!(
    ///
    /// See Also
    /// --------
    /// CashOrNothingOption : Supported contract.
    /// AssetOrNothingOption : Supported contract.
    ///
    /// Notes
    /// -----
    /// Provides node delta/gamma for its product family; other Greeks are absent.
    ///
    CoxRossRubinsteinDigitalEngine,
    digital::DigitalInstrument<'_>,
    digital::with_digital
);
binomial_engine!(
    ///
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    ///
    /// Notes
    /// -----
    /// Provides node delta/gamma for its product family; other Greeks are absent.
    ///
    CoxRossRubinsteinBarrierEngine,
    PyRef<'_, BarrierOption>,
    with_contract
);
