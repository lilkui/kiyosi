macro_rules! engine_methods {
    ($name:ident, $($method:item),* $(,)?) => {
        #[pyo3::pymethods]
        impl $name {
            $($method)*
        }
    };
}

macro_rules! with_contract {
    ($instrument:expr, $option:ident, $body:expr) => {{
        let $option = &$instrument.0;
        $body
    }};
}

// Expand methods before PyO3 parses the impl, including Monte Carlo's settings methods.
macro_rules! numerical_greeks_methods {
    ($instrument:ty,
     |$engine:ident, $py:ident, $option:ident, $context:ident, $request:ident, $shifts:ident| $body:block,
     [$(#[$extra_doc:meta])*],
     $methods:ident!($($args:tt)*)) => {
        $methods! {
            $($args)*,
            /// Returns the price and selected sensitivities.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     Supported contract; see the engine class for accepted types.
            /// context : PricingContext
            ///     Market snapshot and valuation instant.
            /// greeks : str or iterable of str
            ///     Greek names or "all" to request every measure; duplicates are ignored.
            /// spot_shift : float or None, optional
            ///     Positive finite absolute spot bump; None (default) selects 0.01 spot units.
            /// volatility_shift : float, optional
            ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
            /// rate_shift : float, optional
            ///     Positive finite parallel rate bump in decimal units; defaults to 0.0001.
            ///     Applied separately to the risk-free curve for rho and the dividend-yield
            ///     curve for dividend rho.
            /// time_shift_days : int, optional
            ///     Positive forward/backward time bump in calendar days; defaults to 1.
            ///     All controls are validated, including those unused by the estimator. The spot bump
            ///     controls terminal-payoff stencils at expiry.
            ///
            /// Returns
            /// -------
            /// PricingResult
            ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
            ///     with time Greeks absent at expiry; other terminal Greeks follow the payoff.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If requests, shifts, pricing or a feasible bumped valuation fails.
            $(#[$extra_doc])*
            #[pyo3(signature = (instrument, context, greeks, *, spot_shift=None, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
            #[expect(clippy::too_many_arguments, reason = "explicit Python pricing controls")]
            ///
            /// See Also
            /// --------
            /// numerical_greeks : Uniform numerical bump controls for supported BSM pairs.
            ///     Heston and local-volatility models are unsupported by this helper.
            fn price_with_greeks(
                &self,
                py: pyo3::Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &$crate::market::PricingContext,
                greeks: &pyo3::Bound<'_, pyo3::PyAny>,
                spot_shift: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
                #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
                #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
            ) -> pyo3::PyResult<$crate::risk::PricingResult> {
                let $request = $crate::risk::greeks_request(greeks)?;
                let $shifts = kiyosi::NumericalShiftSettings {
                    spot_shift: spot_shift.map(crate::conversions::real).transpose()?, volatility_shift, rate_shift, time_shift_days,
                };
                let $engine = self;
                let $py = py;
                let $option = instrument;
                let $context = context;
                $body
            }
        }
    };
}

pub(crate) mod analytic;
pub(crate) mod binomial;
pub(crate) mod finite_difference;
pub(crate) mod monte_carlo;
pub(crate) mod quadrature;

#[derive(Clone, Copy)]
pub(crate) enum ScalarRequest {
    Price,
    ImpliedVolatility {
        observed_price: f64,
        settings: kiyosi::ImpliedVolatilitySettings,
    },
}

pub(crate) trait PricingRequest<R>: Copy + Sync {
    fn is_price(self) -> bool {
        false
    }
    fn evaluate<I: kiyosi::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<R>;
}

impl PricingRequest<f64> for ScalarRequest {
    fn is_price(self) -> bool {
        matches!(self, Self::Price)
    }

    fn evaluate<I: kiyosi::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<f64> {
        match self {
            Self::Price => engine.price(instrument, context),
            Self::ImpliedVolatility {
                observed_price,
                settings,
            } => kiyosi::implied_volatility(engine, instrument, context, observed_price, settings),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct NumericalGreeksRequest {
    pub request: kiyosi::GreeksRequest,
    pub settings: kiyosi::NumericalShiftSettings,
}

#[derive(Clone, Copy)]
pub(crate) struct StatisticsRequest;

impl PricingRequest<kiyosi::MonteCarloEstimate> for StatisticsRequest {
    fn evaluate<I: kiyosi::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<kiyosi::MonteCarloEstimate> {
        engine.price_with_statistics(instrument, context)
    }
}

impl PricingRequest<kiyosi::PricingResult> for NumericalGreeksRequest {
    fn evaluate<I: kiyosi::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<kiyosi::PricingResult> {
        kiyosi::numerical_greeks(engine, instrument, context, self.request, self.settings)
    }
}
