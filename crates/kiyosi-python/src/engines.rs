use pyo3::prelude::*;

// Expand the complete method block before PyO3 parses the implementation.
macro_rules! engine_methods {
    (@short $option:ident, $context:ident) => {
        $crate::engines::ShortPricing::Never
    };
    (@short $option:ident, $context:ident, $short:path) => {
        $short($option, &$context.0)
    };
    (@evaluate $name:ident, $instrument:ty, $with:path $(, $short:path)?) => {
        impl $name {
            pub(crate) fn evaluate<R: Send>(
                &self,
                py: pyo3::Python<'_>,
                instrument: $instrument,
                context: &$crate::market::PricingContext,
                request: impl $crate::engines::PricingRequest<R>,
            ) -> pyo3::PyResult<R> {
                $with!(&instrument, option, $crate::engines::evaluate(
                    py, &self.0, option, context, request,
                    engine_methods!(@short option, context $(, $short)?),
                ))
            }
        }
    };
    (@price $name:ident, $instrument:ty, $greeks_doc:tt, [$(#[$price_extra:meta])*], {$($method:item)*}) => {
        engine_methods!(
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
            $(#[$price_extra])*
            $name, $instrument, $greeks_doc, {$($method)*}
        );
    };
    ($(#[$price_doc:meta])* $name:ident, $instrument:ty, [$(#[$extra_doc:meta])*], {$($method:item)*}) => {
        #[pyo3::pymethods]
        impl $name {
            $($method)*
            $(#[$price_doc])*
            fn price(
                &self,
                py: pyo3::Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &$crate::market::PricingContext,
            ) -> pyo3::PyResult<f64> {
                self.evaluate(py, instrument, context, $crate::engines::ScalarRequest::Price)
            }
            /// Return the Greek estimation method.
            ///
            /// Checks model applicability; pricing validates life and history.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     Contract supported by this engine.
            /// context : PricingContext
            ///     Market snapshot.
            ///
            /// Returns
            /// -------
            /// str
            ///     "native", "numerical", "mixed", "native_with_spot_control", "terminal",
            ///     or "unsupported".
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If the model is unsupported by this engine.
            ///
            fn greek_method(
                &self,
                py: pyo3::Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &$crate::market::PricingContext,
            ) -> pyo3::PyResult<&'static str> {
                Ok(self.evaluate(py, instrument, context, $crate::engines::GreekMethodRequest)?.name())
            }
            /// Return the price and selected sensitivities using engine estimators.
            ///
            /// Omitted controls or Ellipsis use shared defaults. Shifts affect numerical
            /// fallbacks, event checks and terminal stencils; native estimators and
            /// engine/model Greek coverage are preserved. Every supplied control is
            /// validated.
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
            /// volatility_shift : float or None, optional
            ///     Positive decimal volatility bump. None selects the smaller of 0.0001
            ///     and half the current volatility.
            /// rate_shift : float, optional
            ///     Positive decimal rate bump; defaults to 0.0001.
            /// time_shift : datetime.timedelta or None, optional
            ///     Positive whole-microsecond bump; None selects an automatic time step.
            ///
            /// Returns
            /// -------
            /// PricingResult
            ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
            ///     with time Greeks absent at expiry; other terminal Greeks follow the
            ///     payoff.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If controls are invalid, pricing fails or a feasible bumped valuation
            ///     fails.
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
                let request = $crate::engines::NativeGreeksRequest::from_python(
                    greeks, spot_shift, volatility_shift, rate_shift, time_shift,
                )?;
                Ok($crate::risk::PricingResult(self.evaluate(py, instrument, context, request)?))
            }
        }
    };
}

macro_rules! with_contract {
    ($instrument:expr, $option:ident, $body:expr) => {{
        let $option = &$instrument.0;
        $body
    }};
}

pub(crate) mod analytic;
pub(crate) mod binomial;
pub(crate) mod finite_difference;
pub(crate) mod monte_carlo;
pub(crate) mod quadrature;

// Curve scans cost O(nodes); larger curves go through the cancellable worker.
const SHORT_CURVE_NODES: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortPricing {
    Never,
    WholeCurves,
    Scalar,
    ScalarOnly,
}

impl From<bool> for ShortPricing {
    fn from(short: bool) -> Self {
        if short {
            Self::WholeCurves
        } else {
            Self::Never
        }
    }
}

impl ShortPricing {
    fn allows_inline<R>(
        self,
        request: impl PricingRequest<R>,
        context: &kiyosi::PricingContext,
    ) -> bool {
        if matches!(self, Self::Scalar | Self::ScalarOnly) && request.is_scalar_price() {
            return true;
        }
        let model = context.model_parameters();
        let curve_nodes = model
            .risk_free_curve()
            .nodes()
            .len()
            .max(model.dividend_curve().nodes().len());
        request.is_short(
            matches!(self, Self::WholeCurves | Self::Scalar) && curve_nodes <= SHORT_CURVE_NODES,
        )
    }
}

fn expiry_short_pricing(
    option: &impl kiyosi::instruments::OptionContract,
    context: &kiyosi::PricingContext,
) -> ShortPricing {
    if context.valuation_time() == option.expiry_time() {
        ShortPricing::Scalar
    } else {
        ShortPricing::Never
    }
}

pub(crate) fn evaluate<I: kiyosi::instruments::OptionContract + Sync, R: Send>(
    py: Python<'_>,
    engine: &(impl kiyosi::PricingEngine<I> + Sync),
    instrument: &I,
    context: &crate::market::PricingContext,
    request: impl PricingRequest<R>,
    short_pricing: ShortPricing,
) -> PyResult<R> {
    let context = &context.0;
    let operation = || request.evaluate(engine, instrument, context);
    let result = if short_pricing.allows_inline(request, context) {
        py.check_signals()?;
        let result = operation();
        py.check_signals()?;
        result
    } else {
        crate::cancellation::run(py, operation)?
    };
    crate::error::domain(result)
}

#[derive(Clone, Copy)]
pub(crate) enum ScalarRequest {
    Price,
    ImpliedVolatility {
        observed_price: f64,
        settings: kiyosi::ImpliedVolatilitySettings,
    },
}

pub(crate) trait PricingRequest<R>: Copy + Sync {
    fn is_scalar_price(self) -> bool {
        false
    }
    fn is_short(self, _short_pricing: bool) -> bool {
        false
    }
    fn evaluate<I: kiyosi::instruments::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<R>;
}

impl PricingRequest<f64> for ScalarRequest {
    fn is_scalar_price(self) -> bool {
        matches!(self, Self::Price)
    }

    fn is_short(self, short_pricing: bool) -> bool {
        short_pricing && self.is_scalar_price()
    }

    fn evaluate<I: kiyosi::instruments::OptionContract>(
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
pub(crate) struct NumericalGreeksRequest(pub NativeGreeksRequest);

#[derive(Clone, Copy)]
pub(crate) struct NativeGreeksRequest {
    pub request: kiyosi::GreeksRequest,
    pub settings: kiyosi::NumericalShiftSettings,
}

impl NativeGreeksRequest {
    pub(crate) fn from_python(
        greeks: &Bound<'_, PyAny>,
        spot_shift: Option<&Bound<'_, PyAny>>,
        volatility_shift: Option<&Bound<'_, PyAny>>,
        rate_shift: Option<&Bound<'_, PyAny>>,
        time_shift: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let defaults = kiyosi::NumericalShiftSettings::default();
        Ok(Self {
            request: crate::risk::greeks_request(greeks)?,
            settings: kiyosi::NumericalShiftSettings {
                spot_shift: spot_shift
                    .map(crate::conversions::optional_real)
                    .transpose()?
                    .unwrap_or(defaults.spot_shift),
                volatility_shift: volatility_shift
                    .map(crate::conversions::optional_real)
                    .transpose()?
                    .unwrap_or(defaults.volatility_shift),
                rate_shift: rate_shift
                    .map(crate::conversions::real)
                    .transpose()?
                    .unwrap_or(defaults.rate_shift),
                time_shift: time_shift
                    .map(crate::conversions::optional_duration)
                    .transpose()?
                    .unwrap_or(defaults.time_shift),
            },
        })
    }
}

impl PricingRequest<kiyosi::PricingResult> for NativeGreeksRequest {
    fn is_short(self, short_pricing: bool) -> bool {
        short_pricing
    }

    fn evaluate<I: kiyosi::instruments::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<kiyosi::PricingResult> {
        engine.price_with_greeks_with_settings(instrument, context, self.request, self.settings)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct StatisticsRequest;

#[derive(Clone, Copy)]
pub(crate) struct SeededPriceRequest;

impl PricingRequest<(f64, Option<u64>)> for SeededPriceRequest {
    fn evaluate<I: kiyosi::instruments::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<(f64, Option<u64>)> {
        engine.price_with_seed(instrument, context)
    }
}

impl PricingRequest<kiyosi::engines::MonteCarloEstimate> for StatisticsRequest {
    fn evaluate<I: kiyosi::instruments::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<kiyosi::engines::MonteCarloEstimate> {
        engine.price_with_statistics(instrument, context)
    }
}

impl PricingRequest<kiyosi::PricingResult> for NumericalGreeksRequest {
    fn evaluate<I: kiyosi::instruments::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<kiyosi::PricingResult> {
        kiyosi::numerical_greeks(engine, instrument, context, self.0.request, self.0.settings)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct GreekMethodRequest;

impl PricingRequest<kiyosi::GreekMethod> for GreekMethodRequest {
    fn is_short(self, _short_pricing: bool) -> bool {
        true
    }

    fn evaluate<I: kiyosi::instruments::OptionContract>(
        self,
        engine: &impl kiyosi::PricingEngine<I>,
        instrument: &I,
        context: &kiyosi::PricingContext,
    ) -> kiyosi::Result<kiyosi::GreekMethod> {
        engine.greek_method(instrument, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiyosi::{
        AmericanOption, BlackScholesMertonParameters, Date, DigitalOption, EuropeanOption,
        OptionType,
    };

    #[test]
    fn terminal_prices_dispatch_inline() -> kiyosi::Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let expiry = start.add_days(365)?;
        let european = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
        let american = AmericanOption::new(OptionType::Put, 100.0, start, expiry)?;
        let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, expiry)?;
        for (valuation, inline) in [
            (expiry.add_micros(-1)?, false),
            (expiry, true),
            (expiry.add_micros(1)?, false),
        ] {
            let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
            let context = kiyosi::PricingContext::new(model, 100.0, valuation)?;
            for policy in [
                expiry_short_pricing(&european, &context),
                expiry_short_pricing(&american, &context),
                expiry_short_pricing(&digital, &context),
            ] {
                assert_eq!(policy.allows_inline(ScalarRequest::Price, &context), inline);
                assert!(!policy.allows_inline(SeededPriceRequest, &context));
                assert!(!policy.allows_inline(StatisticsRequest, &context));
            }
        }
        Ok(())
    }

    #[test]
    fn analytic_scalar_price_is_inline_with_dense_curves() -> kiyosi::Result<()> {
        let start = kiyosi::Date::new(2025, 1, 1)?.start_of_day();
        let times = (1..=1025)
            .map(|day| start.add_days(day))
            .collect::<kiyosi::Result<Vec<_>>>()?;
        let factors = vec![1.0; times.len()];
        let curve = kiyosi::RateTermStructure::new(start, times, factors)?;
        let model = kiyosi::BlackScholesMertonParameters::new(curve, 0.0, 0.2)?;
        let context = kiyosi::PricingContext::new(model, 100.0, start)?;
        assert!(ShortPricing::Scalar.allows_inline(ScalarRequest::Price, &context));
        assert!(!ShortPricing::WholeCurves.allows_inline(ScalarRequest::Price, &context));
        assert!(ShortPricing::Never.allows_inline(GreekMethodRequest, &context));
        let greeks = NativeGreeksRequest {
            request: kiyosi::GreeksRequest::all(),
            settings: Default::default(),
        };
        assert!(!ShortPricing::Scalar.allows_inline(greeks, &context));
        Ok(())
    }

    #[test]
    fn bounded_metadata_is_short_independently_of_pricing_but_solvers_are_not() {
        let greeks = NativeGreeksRequest {
            request: kiyosi::GreeksRequest::all(),
            settings: Default::default(),
        };
        let solver = ScalarRequest::ImpliedVolatility {
            observed_price: 10.0,
            settings: kiyosi::ImpliedVolatilitySettings::new(0.1, 0.3),
        };
        for short_pricing in [false, true] {
            assert!(GreekMethodRequest.is_short(short_pricing));
            assert!(!solver.is_short(short_pricing));
            assert!(!NumericalGreeksRequest(greeks).is_short(short_pricing));
            assert_eq!(ScalarRequest::Price.is_short(short_pricing), short_pricing);
            assert_eq!(greeks.is_short(short_pricing), short_pricing);
            assert!(!SeededPriceRequest.is_short(short_pricing));
            assert!(!StatisticsRequest.is_short(short_pricing));
        }
    }
}
