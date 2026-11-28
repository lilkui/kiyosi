use crate::conversions::{integer, real};
use crate::engines::binomial::{VanillaInstrument, with_vanilla};
use crate::error::domain;
use crate::instruments::{accumulator::Accumulator, autocallable, barrier::BarrierOption, digital};
use crate::market::PricingContext;
use pyo3::prelude::*;

macro_rules! quadrature_engine {
    (@short QuadratureVanillaEngine, $engine:expr, $instrument:expr, $context:expr) => {
        short_vanilla(
            matches!($instrument, VanillaInstrument::European(_)),
            $engine.0.settings(),
            &$context.0,
        )
    };
    (@short $name:ident, $engine:expr, $instrument:expr, $context:expr) => { false };
    ($(#[$doc:meta])* $name:ident, $instrument:ty $(,)?) => {
        quadrature_engine!($(#[$doc])* $name, $instrument, with_contract);
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty,
     $with:path $(,)?) => {
        quadrature_engine!($(#[$doc])* $name, $instrument, $with, QuadratureSettings);
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty,
     $with:path, $settings:ident $(, $time:ident)? $(, { $($extra:tt)* })? $(,)?) => {
        /// Flat-rate quadrature pricing.
        ///
        /// Parameters
        /// ----------
        /// asset_step_count : int, optional
        ///     Base number of spatial intervals, at least 4; defaults to 512. Event
        ///     boundaries may add intervals; Simpson panels round up to even.
        $(#[doc = concat!(stringify!($time), " : int, optional\n    American exercise intervals, at least 1; defaults to 200. Ignored for European pricing.")])?
        /// integration_standard_deviations : float, optional
        ///     Positive finite integration extent in standard deviations; defaults to
        ///     8.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If asset_step_count is below 4 or integration_standard_deviations is
        ///     nonpositive or non-finite.
        ///     Gaussian grid pricing also raises this if the estimated grid or
        ///     transition workspace exceeds 256 MiB, or rollback exceeds one
        ///     billion node and cell visits.
        ///     Simpson integration, when used, rejects more than 250 million panels
        ///     across all sections.
        $(#[doc = concat!("    Also raised if ", stringify!($time), " is below 1.")])?
        ///
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        engine_methods!(@price $name, $instrument,
            [
                ///
                /// Notes
                /// -----
                /// Before expiry, requested Greeks use scalar numerical differences with
                /// the request's shift controls. Omitted controls use engine defaults.
                ///
            ],
            [],
            {
                $($($extra)*)?

                #[classattr]
                const _STATE_FIELDS: &'static [&'static str] = &[
                    "asset_step_count", $(stringify!($time),)? "integration_standard_deviations",
                ];

                #[new]
                #[pyo3(signature = (*, asset_step_count=kiyosi::$settings::default().asset_step_count, $($time=kiyosi::$settings::default().$time,)? integration_standard_deviations=kiyosi::$settings::default().integration_standard_deviations))]
                fn new(
                    #[pyo3(from_py_with = integer)] asset_step_count: i32,
                    $(#[pyo3(from_py_with = integer)] $time: i32,)?
                    #[pyo3(from_py_with = real)] integration_standard_deviations: f64,
                ) -> PyResult<Self> {
                    Ok(Self(domain(kiyosi::$name::new(
                        kiyosi::$settings {
                            asset_step_count,
                            $($time,)?
                            integration_standard_deviations,
                        },
                    ))?))
                }

                /// Return the configured number of spatial intervals.
                #[getter]
                fn asset_step_count(&self) -> i32 {
                    self.0.settings().asset_step_count
                }

                $(
                    /// Return the configured American exercise interval count.
                    #[getter]
                    fn $time(&self) -> i32 {
                        self.0.settings().$time
                    }
                )?
                /// Return the integration extent in standard deviations.
                #[getter]
                fn integration_standard_deviations(&self) -> f64 {
                    self.0.settings().integration_standard_deviations
                }
            }
        );
        impl $name {
            pub(crate) fn evaluate<R: Send>(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                request: impl crate::engines::PricingRequest<R>,
            ) -> PyResult<R> {
                let short = quadrature_engine!(@short $name, self, instrument, context);
                $with!(&instrument, option, crate::engines::evaluate(
                    py, &self.0, option, context, request, short.into(),
                ))
            }
        }
    };
}

fn short_vanilla(
    european: bool,
    settings: kiyosi::QuadratureVanillaSettings,
    context: &kiyosi::PricingContext,
) -> bool {
    let bounded = if european {
        settings.asset_step_count <= kiyosi::QuadratureVanillaSettings::default().asset_step_count
    } else {
        settings.time_step_count == 1
    };
    bounded
        && context.model_parameters().as_bsm().is_ok_and(|model| {
            model.risk_free_curve().nodes().is_empty() && model.dividend_curve().nodes().is_empty()
        })
}

quadrature_engine!(
    /// See Also
    /// --------
    /// EuropeanOption : Supported contract.
    /// AmericanOption : Supported contract.
    ///
    /// Notes
    /// -----
    /// Uses Simpson integration for European options and Gaussian backward
    /// induction with piecewise cubic continuation interpolation for American
    /// options. American calls with nonnegative rates and nonpositive dividend
    /// yields use the analytic European price. With one exercise interval,
    /// American pricing takes the greater of immediate exercise and the analytic
    /// European price without using a spatial grid. With more intervals both
    /// values are lower bounds on the rollback price. Requires flat-rate BSM parameters.
    /// Check spatial, time and
    /// integration-extent convergence separately; fixed-grid completion does not
    /// estimate error. Higher Greeks near exercise boundaries also require bump
    /// convergence checks.
    ///
    QuadratureVanillaEngine,
    VanillaInstrument<'_>,
    with_vanilla,
    QuadratureVanillaSettings,
    time_step_count,
    {
        /// Price American options at multiple strikes on one continuation curve.
        ///
        /// Parameters
        /// ----------
        /// instrument : AmericanOption
        ///     Template for option direction and dates.
        /// context : PricingContext
        ///     Flat-rate Black–Scholes–Merton snapshot.
        /// strikes : iterable of float
        ///     Finite positive strikes.
        ///
        /// Returns
        /// -------
        /// tuple of float
        ///     Prices in strike order, preserving duplicates.
        ///
        /// Raises
        /// ------
        /// KiyosiError
        ///     If contract, market, strike or pricing validation fails.
        /// OSError
        ///     If interrupted.
        #[pyo3(signature = (instrument, context, *, strikes))]
        #[expect(
            clippy::needless_pass_by_value,
            reason = "PyO3 owns the extracted inputs."
        )]
        fn price_strikes<'py>(
            &self,
            py: Python<'py>,
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
                '_,
                crate::instruments::american::AmericanOption,
            >,
            context: &PricingContext,
            #[pyo3(from_py_with = crate::conversions::reals)] strikes: Vec<f64>,
        ) -> PyResult<Bound<'py, pyo3::types::PyTuple>> {
            let instrument = &instrument.0;
            let prices = domain(crate::cancellation::run(py, || {
                self.0.price_strikes(instrument, &context.0, &strikes)
            })?)?;
            crate::conversions::python_tuple(py, prices, Ok)
        }
    },
);

quadrature_engine!(
    /// See Also
    /// --------
    /// CashOrNothingOption : Supported European cash payment.
    /// AssetOrNothingOption : Supported European asset payment.
    ///
    /// Notes
    /// -----
    /// Uses Simpson integration on the paying side of the strike under flat-rate
    /// BSM. The extent is centered on the payment-weighted normal density; omitted
    /// tails are zero. Equality at expiry pays zero. Check step-count and extent
    /// convergence separately; completion provides no error estimate. Numerical
    /// Greeks also require bump convergence checks.
    ///
    QuadratureDigitalEngine,
    digital::DigitalInstrument<'_>,
    digital::with_digital,
);

quadrature_engine!(
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    ///
    QuadratureBarrierEngine,
    PyRef<'_, BarrierOption>,
);

quadrature_engine!(
    /// See Also
    /// --------
    /// Accumulator : Supported contract.
    ///
    QuadratureAccumulatorEngine,
    PyRef<'_, Accumulator>,
);

quadrature_engine!(
    /// See Also
    /// --------
    /// SnowballOption : Supported contract.
    /// BinarySnowballOption : Supported contract.
    /// TernarySnowballOption : Supported contract.
    /// PhoenixOption : Supported contract.
    ///
    /// Notes
    /// -----
    /// Preserves cash and asset payments below the spatial grid, including
    /// zero-lower-strike losses after large spot declines.
    ///
    QuadratureAutocallableEngine,
    autocallable::AutocallableInstrument<'_>,
    autocallable::with_note,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_european_and_single_interval_american_quadrature_use_short_calls()
    -> kiyosi::Result<()> {
        let start = kiyosi::Date::new(2025, 1, 1)?.start_of_day();
        let end = start.add_days(365)?;
        let model = kiyosi::BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
        let context = kiyosi::PricingContext::new(model.clone(), 100.0, start)?;
        let settings = kiyosi::QuadratureVanillaSettings::default();
        let one_interval = kiyosi::QuadratureVanillaSettings {
            time_step_count: 1,
            asset_step_count: i32::MAX,
            ..settings
        };
        assert!(short_vanilla(true, settings, &context));
        assert!(!short_vanilla(false, settings, &context));
        assert!(short_vanilla(false, one_interval, &context));
        assert!(!short_vanilla(
            true,
            kiyosi::QuadratureVanillaSettings {
                asset_step_count: settings.asset_step_count + 1,
                ..settings
            },
            &context,
        ));
        let curve = kiyosi::RateTermStructure::new(start, vec![end], vec![1.0])?;
        for changes in [
            kiyosi::BlackScholesMertonParameterChanges {
                risk_free_rate: Some(curve.clone().into()),
                ..Default::default()
            },
            kiyosi::BlackScholesMertonParameterChanges {
                dividend_yield: Some(curve.into()),
                ..Default::default()
            },
        ] {
            let context = context.replace(kiyosi::PricingContextChanges {
                model_parameters: Some(model.replace(changes)?.into()),
                ..Default::default()
            })?;
            assert!(!short_vanilla(true, settings, &context));
            assert!(!short_vanilla(false, one_interval, &context));
        }
        Ok(())
    }
}
