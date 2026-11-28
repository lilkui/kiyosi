use crate::conversions::integer;
use crate::engines::{PricingRequest, ShortPricing};
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

fn short_pricing(
    step_count: i32,
    terminal: bool,
    context: &kiyosi::PricingContext,
) -> ShortPricing {
    // Terminal payoffs use a linear-work fallback at 256 steps and above.
    let bounded = if terminal {
        step_count <= 64 || (256..=1024).contains(&step_count)
    } else {
        step_count <= 32
    };
    let model = context.model_parameters();
    if bounded
        && model.risk_free_curve().nodes().is_empty()
        && model.dividend_curve().nodes().is_empty()
    {
        ShortPricing::Scalar
    } else {
        ShortPricing::Never
    }
}

macro_rules! binomial_engine {
    // Barrier contracts can scan a monitoring schedule even on a short tree.
    (@short CoxRossRubinsteinBarrierEngine, $engine:expr, $context:expr, $instrument:expr) => {
        ShortPricing::Never
    };
    (@short CoxRossRubinsteinVanillaEngine, $engine:expr, $context:expr, $instrument:expr) => {
        short_pricing(
            $engine.settings().step_count,
            matches!($instrument, VanillaInstrument::European(_)),
            $context,
        )
    };
    (@short CoxRossRubinsteinDigitalEngine, $engine:expr, $context:expr, $instrument:expr) => {
        short_pricing($engine.settings().step_count, true, $context)
    };
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
            ],
            {
                #[classattr]
                const _STATE_FIELDS: &'static [&'static str] = &["step_count"];

                #[new]
                #[pyo3(signature = (*, step_count=kiyosi::engines::BinomialSettings::default().step_count))]
                fn new(
                    #[pyo3(from_py_with = integer)] step_count: i32,
                ) -> PyResult<Self> {
                    Ok(Self(domain(kiyosi::$name::new(kiyosi::engines::BinomialSettings { step_count }))?))
                }
            }
        );
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty, $with:path,
     [$(#[$greeks_doc:meta])*], {$($extra:tt)*}) => {
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
                /// Return the configured number of binomial time steps.
                #[getter]
                fn step_count(&self) -> i32 {
                    self.0.settings().step_count
                }
                $($extra)*
            }
        );
        impl $name {
            pub(crate) fn evaluate<R: Send>(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &crate::market::PricingContext,
                request: impl PricingRequest<R>,
            ) -> PyResult<R> {
                let short = binomial_engine!(@short $name, self.0, &context.0, &instrument);
                $with!(&instrument, option, crate::engines::evaluate(
                    py, &self.0, option, context, request, short,
                ))
            }
        }
    };
}

binomial_engine!(
    ///
    /// Notes
    /// -----
    /// Accepts EuropeanOption and AmericanOption.
    /// Set rollback_tolerance to a positive absolute tolerance in price units
    /// to bound discarded lattice tails in ordinary American option pricing,
    /// Greeks, and implied volatility. The default, None, uses full rollback.
    /// Grid discretization and rounding errors are separate. Greek stencils
    /// can tighten the truncation bound.
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
        #[classattr]
        const _STATE_FIELDS: &'static [&'static str] = &["step_count", "rollback_tolerance"];

        #[new]
        #[pyo3(signature = (*, step_count=kiyosi::engines::BinomialVanillaSettings::default().step_count,
            rollback_tolerance=kiyosi::engines::BinomialVanillaSettings::default().rollback_tolerance))]
        fn new(
            #[pyo3(from_py_with = integer)] step_count: i32,
            #[pyo3(from_py_with = crate::conversions::optional_real)] rollback_tolerance: Option<f64>,
        ) -> PyResult<Self> {
            Ok(Self(domain(kiyosi::CoxRossRubinsteinVanillaEngine::new(
                kiyosi::engines::BinomialVanillaSettings { step_count, rollback_tolerance }
            ))?))
        }

        /// Return the American option lattice truncation tolerance in price units.
        #[getter]
        fn rollback_tolerance(&self) -> Option<f64> {
            self.0.settings().rollback_tolerance
        }

        /// Price vanilla options in input order on one market snapshot.
        ///
        /// Parameters
        /// ----------
        /// instruments : iterable of EuropeanOption or AmericanOption
        ///     Contracts to price. An empty iterable is valid.
        /// context : PricingContext
        ///     Shared model inputs, spot and valuation time.
        ///
        /// Returns
        /// -------
        /// tuple of float
        ///     Prices in input order, preserving duplicates.
        ///
        /// Raises
        /// ------
        /// KiyosiError
        ///     If contract applicability, pricing validation or computation fails.
        /// OSError
        ///     If interrupted or the pricing worker cannot start.
        ///
        /// Notes
        /// -----
        /// Pricing runs in one cancellable request with the GIL released.
        /// Compatible contracts can share lattice geometry and run in parallel.
        /// The engine's rollback tolerance applies to American contracts.
        /// The request accepts up to 1,048,576 contracts and caps combined
        /// terminal-node and rollback work at one billion grid updates.
        /// A failed request returns no partial results.
        fn price_many<'py>(
            &self,
            py: Python<'py>,
            instruments: &Bound<'py, PyAny>,
            context: &crate::market::PricingContext,
        ) -> PyResult<Bound<'py, pyo3::types::PyTuple>> {
            let instruments = crate::conversions::collect_values_checked(
                instruments,
                |value| crate::conversions::instrument::<VanillaInstrument<'py>>(value),
                |count| domain(kiyosi::binding_support::validate_contract_batch_size(count)),
            )?;
            if instruments.is_empty() {
                return Ok(pyo3::types::PyTuple::empty(py));
            }
            let contracts: Vec<_> = instruments.iter().map(|instrument| match instrument {
                VanillaInstrument::European(option) => kiyosi::engines::BinomialVanilla::European(&option.0),
                VanillaInstrument::American(option) => kiyosi::engines::BinomialVanilla::American(&option.0),
            }).collect();
            let context = &context.0;
            let prices = domain(crate::cancellation::run(py, || {
                self.0.price_many(&contracts, context)
            })?)?;
            crate::conversions::python_tuple(py, prices, Ok)
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
    /// Reflected trees and sparse schedules can exceed 16,000 steps when
    /// their rollback fits the work budget.
    /// Provides node delta/gamma for its product family; other Greeks are absent.
    ///
    CoxRossRubinsteinBarrierEngine,
    PyRef<'_, BarrierOption>,
    with_contract
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::ScalarRequest;

    #[test]
    fn short_trees_dispatch_only_bounded_scalar_prices_inline() -> kiyosi::Result<()> {
        let start = kiyosi::Date::new(2025, 1, 1)?.start_of_day();
        let flat = kiyosi::BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
        let context = kiyosi::PricingContext::new(flat, 100.0, start)?;
        let inline = |steps, terminal| {
            short_pricing(steps, terminal, &context).allows_inline(ScalarRequest::Price, &context)
        };
        for steps in [33, 64, 256, 1024] {
            assert!(inline(steps, true), "{steps} terminal steps");
        }
        for steps in [65, 255, 1025] {
            assert!(!inline(steps, true), "{steps} terminal steps");
        }
        assert!(inline(32, false));
        assert!(!inline(33, false));
        assert!(!inline(256, false));

        let curve = kiyosi::RateTermStructure::new(start, vec![start.add_days(365)?], vec![0.95])?;
        let model = kiyosi::BlackScholesMertonParameters::new(curve, 0.02, 0.2)?;
        let context = kiyosi::PricingContext::new(model, 100.0, start)?;
        assert!(!short_pricing(3, true, &context).allows_inline(ScalarRequest::Price, &context));
        Ok(())
    }
}
