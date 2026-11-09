use crate::conversions::{integer, real};
use crate::engines::ScalarRequest;
use crate::engines::binomial::{VanillaInstrument, with_vanilla};
use crate::error::domain;
use crate::instruments::{accumulator::Accumulator, autocallable, barrier::BarrierOption, digital};
use crate::market::PricingContext;
use pyo3::prelude::*;

macro_rules! quadrature_engine {
    ($(#[$doc:meta])* $name:ident, $instrument:ty $(,)?) => {
        quadrature_engine!($(#[$doc])* $name, $instrument, with_contract);
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty,
     $with:path $(,)?) => {
        quadrature_engine!($(#[$doc])* $name, $instrument, $with, QuadratureSettings);
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty,
     $with:path, $settings:ident $(, $time:ident)? $(,)?) => {
        /// Flat-rate quadrature pricing.
        ///
        /// Parameters
        /// ----------
        /// asset_step_count : int, optional
        ///     Base number of spatial intervals, at least 4; defaults to 512. Event
        ///     boundaries may add intervals; Simpson panels round up to even.
        ///
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
        ///
        $(#[doc = concat!("    Also raised if ", stringify!($time), " is below 1.")])?
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        numerical_greeks_methods!(
            $instrument,
            [
                ///
                /// Notes
                /// -----
                /// Before expiry requested Greeks use scalar numerical differences and the
                /// engine defaults. Use numerical_greeks for configurable scalar differences.
                ///
            ],
            engine_methods!(
                $name,
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
                },
                /// Return the configured number of spatial intervals.
                #[getter]
                fn asset_step_count(&self) -> i32 {
                    self.0.settings().asset_step_count
                },
                $(
                    /// Return the configured American exercise interval count.
                    #[getter]
                    fn $time(&self) -> i32 {
                        self.0.settings().$time
                    },
                )?
                /// Return the integration extent in standard deviations.
                #[getter]
                fn integration_standard_deviations(&self) -> f64 {
                    self.0.settings().integration_standard_deviations
                },
                /// Return the price.
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
                ///
                fn price(
                    &self,
                    py: Python<'_>,
                    #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                    context: &PricingContext,
                ) -> PyResult<f64> {
                    self.evaluate(py, instrument, context, ScalarRequest::Price)
                }
            )
        );
        impl $name {
            pub(crate) fn evaluate<R: Send>(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                request: impl crate::engines::PricingRequest<R>,
            ) -> PyResult<R> {
                let engine = self.0;
                let context = &context.0;
                $with!(&instrument, option, domain(request.run(py,
                    kiyosi::OptionContract::expiry_time(option) == context.valuation_time(), || {
                    request.evaluate(&engine, option, context)
                })?))
            }
        }
    };
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
    /// options. Requires flat-rate BSM parameters. Check spatial, time and
    /// integration-extent convergence separately; fixed-grid completion does not
    /// estimate error. Higher Greeks near exercise boundaries also require bump
    /// convergence checks.
    ///
    QuadratureVanillaEngine,
    VanillaInstrument<'_>,
    with_vanilla,
    QuadratureVanillaSettings,
    time_step_count,
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
