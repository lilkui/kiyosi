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
    ($instrument:ty, [$(#[$extra_doc:meta])*], $methods:ident!($($args:tt)*)) => {
        $methods! {
            $($args)*,
            /// Return native, numerical, mixed, native_with_spot_control, terminal or unsupported.
            ///
            /// Checks model applicability without pricing or sampling. Life and history
            /// validation remain part of pricing. Use numerical_greeks for scalar bumps.
            fn greek_method(
                &self,
                py: pyo3::Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &$crate::market::PricingContext,
            ) -> pyo3::PyResult<&'static str> {
                Ok(self.evaluate(py, instrument, context, $crate::engines::GreekMethodRequest)?.name())
            },
            /// Return shift-control names potentially used by this Greek calculation path.
            ///
            /// Individual requests may use fewer controls. Every engine accepts the same
            /// request-level controls; omitted controls retain the engine's settings.
            fn active_greek_controls(
                &self,
                py: pyo3::Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &$crate::market::PricingContext,
            ) -> pyo3::PyResult<Vec<&'static str>> {
                Ok(self.evaluate(py, instrument, context, $crate::engines::GreekMethodRequest)?.active_controls().to_vec())
            },
            /// Return the price and selected sensitivities with native estimators.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     Supported contract; see the engine class for accepted types.
            /// context : PricingContext
            ///     Market snapshot and valuation instant.
            /// greeks : str or iterable of str
            ///     Greek names or "all" to request every measure; duplicates are ignored.
            ///     An empty iterable requests price only.
            /// spot_shift : float or None, optional
            ///     Positive absolute spot bump; None selects the product's automatic bump.
            /// volatility_shift, rate_shift : float, optional
            ///     Positive decimal bumps for numerical fallback Greeks.
            /// time_shift : datetime.timedelta or None, optional
            ///     Positive whole-microsecond bump; None selects an automatic time step.
            ///     Omitted controls or Ellipsis retain engine settings, whose shared defaults
            ///     are automatic spot/time bumps and 0.0001 volatility/rate bumps.
            ///     Controls affect numerical fallbacks, event checks and terminal stencils;
            ///     native estimators and engine/model Greek coverage are preserved.
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
            ///     If controls are invalid, pricing fails or a feasible bumped valuation fails.
            /// ValueError
            ///     If a Greek name is unknown.
            /// TypeError
            ///     If representations, Greek names or keyword arguments are invalid.
            $(#[$extra_doc])*
            #[pyo3(signature = (instrument, context, greeks, *, spot_shift=crate::conversions::OMITTED,
                volatility_shift=crate::conversions::OMITTED, rate_shift=crate::conversions::OMITTED,
                time_shift=crate::conversions::OMITTED))]
            #[expect(clippy::too_many_arguments, reason = "uniform keyword-only Python Greek controls")]
            fn price_with_greeks(
                &self,
                py: pyo3::Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &$crate::market::PricingContext,
                greeks: &pyo3::Bound<'_, pyo3::PyAny>,
                #[pyo3(from_py_with = crate::conversions::supplied)] spot_shift: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
                #[pyo3(from_py_with = crate::conversions::supplied)] volatility_shift: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
                #[pyo3(from_py_with = crate::conversions::supplied)] rate_shift: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
                #[pyo3(from_py_with = crate::conversions::supplied)] time_shift: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
            ) -> pyo3::PyResult<$crate::risk::PricingResult> {
                let request = $crate::engines::NativeGreeksRequest {
                    request: $crate::risk::greeks_request(greeks)?,
                    spot_shift: spot_shift.map(|v| if v.is_none() { Ok(None) } else { crate::conversions::real(v).map(Some) }).transpose()?,
                    volatility_shift: volatility_shift.map(crate::conversions::real).transpose()?,
                    rate_shift: rate_shift.map(crate::conversions::real).transpose()?,
                    time_shift: time_shift.map(|v| if v.is_none() { Ok(None) } else { crate::conversions::duration(v).map(Some) }).transpose()?,
                };
                Ok($crate::risk::PricingResult(self.evaluate(py, instrument, context, request)?))
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
    fn is_short(self) -> bool {
        false
    }
    fn run(
        self,
        py: pyo3::Python<'_>,
        operation: impl FnOnce() -> kiyosi::Result<R> + Send,
    ) -> pyo3::PyResult<kiyosi::Result<R>>
    where
        R: Send,
    {
        crate::cancellation::run(py, operation)
    }
    fn evaluate<I: kiyosi::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<R>;
}

impl PricingRequest<f64> for ScalarRequest {
    fn is_short(self) -> bool {
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
#[expect(
    clippy::option_option,
    reason = "omission retains settings; None selects automatic bumps"
)]
pub(crate) struct NativeGreeksRequest {
    pub request: kiyosi::GreeksRequest,
    pub spot_shift: Option<Option<f64>>,
    pub volatility_shift: Option<f64>,
    pub rate_shift: Option<f64>,
    pub time_shift: Option<Option<std::time::Duration>>,
}

impl PricingRequest<kiyosi::PricingResult> for NativeGreeksRequest {
    fn is_short(self) -> bool {
        true
    }

    fn evaluate<I: kiyosi::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<kiyosi::PricingResult> {
        let defaults = engine.greek_settings(instrument);
        engine.price_with_greeks_with_settings(
            instrument,
            context,
            self.request,
            kiyosi::NumericalShiftSettings {
                spot_shift: self.spot_shift.unwrap_or(defaults.spot_shift),
                volatility_shift: self.volatility_shift.unwrap_or(defaults.volatility_shift),
                rate_shift: self.rate_shift.unwrap_or(defaults.rate_shift),
                time_shift: self.time_shift.unwrap_or(defaults.time_shift),
            },
        )
    }
}

#[derive(Clone, Copy)]
pub(crate) struct StatisticsRequest;

#[derive(Clone, Copy)]
pub(crate) struct SeededPriceRequest;

impl PricingRequest<(f64, Option<u64>)> for SeededPriceRequest {
    fn evaluate<I: kiyosi::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<(f64, Option<u64>)> {
        engine.price_with_seed(instrument, context)
    }
}

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

#[derive(Clone, Copy)]
pub(crate) struct GreekMethodRequest;

impl PricingRequest<kiyosi::GreekMethod> for GreekMethodRequest {
    fn run(
        self,
        py: pyo3::Python<'_>,
        operation: impl FnOnce() -> kiyosi::Result<kiyosi::GreekMethod> + Send,
    ) -> pyo3::PyResult<kiyosi::Result<kiyosi::GreekMethod>> {
        py.check_signals()?;
        let result = operation();
        py.check_signals()?;
        Ok(result)
    }

    fn evaluate<I: kiyosi::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<kiyosi::GreekMethod> {
        engine.greek_method(instrument, context)
    }
}
