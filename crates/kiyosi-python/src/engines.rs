use pyo3::prelude::*;

// Expand the complete method block before PyO3 parses the implementation.
macro_rules! engine_methods {
    (@evaluate $name:ident, $instrument:ty, $with:path) => {
        impl $name {
            pub(crate) fn evaluate<R: Send>(
                &self,
                py: pyo3::Python<'_>,
                instrument: $instrument,
                context: &$crate::market::PricingContext,
                request: impl $crate::engines::PricingRequest<R>,
            ) -> pyo3::PyResult<R> {
                $with!(&instrument, option, $crate::engines::evaluate(py, &self.0, option, context, request, false))
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
            /// Return shift controls used by the Greek estimation method.
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
            /// list of str
            ///     Potentially active controls: spot_shift, volatility_shift, rate_shift,
            ///     and time_shift. Individual requests may use fewer controls.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If the model is unsupported by this engine.
            ///
            fn active_greek_controls(
                &self,
                py: pyo3::Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &$crate::market::PricingContext,
            ) -> pyo3::PyResult<Vec<&'static str>> {
                Ok(self.evaluate(py, instrument, context, $crate::engines::GreekMethodRequest)?.active_controls().to_vec())
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

pub(crate) fn evaluate<I: kiyosi::OptionContract + Sync, R: Send>(
    py: Python<'_>,
    engine: &(impl kiyosi::PricingEngine<I> + Sync),
    instrument: &I,
    context: &crate::market::PricingContext,
    request: impl PricingRequest<R>,
    short_pricing: bool,
) -> PyResult<R> {
    let context = &context.0;
    let model = context.model_parameters();
    let curve_nodes = model
        .risk_free_curve()
        .nodes()
        .len()
        .max(model.dividend_curve().nodes().len());
    let operation = || request.evaluate(engine, instrument, context);
    let result = if request.is_short(short_pricing && curve_nodes == 0, curve_nodes) {
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
    fn is_short(self, _short_pricing: bool, _curve_nodes: usize) -> bool {
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
    fn is_short(self, short_pricing: bool, _curve_nodes: usize) -> bool {
        short_pricing && matches!(self, Self::Price)
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
    fn is_short(self, short_pricing: bool, _curve_nodes: usize) -> bool {
        short_pricing
    }

    fn evaluate<I: kiyosi::OptionContract>(
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
        kiyosi::numerical_greeks(engine, instrument, context, self.0.request, self.0.settings)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct GreekMethodRequest;

impl PricingRequest<kiyosi::GreekMethod> for GreekMethodRequest {
    fn is_short(self, _short_pricing: bool, curve_nodes: usize) -> bool {
        // Flat-rate applicability checks may scan every curve node.
        curve_nodes <= 1024
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

#[cfg(test)]
mod tests {
    use super::*;

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
        for curve_nodes in [0, 1, 1024, 1025] {
            for short_pricing in [false, true] {
                let short_pricing = short_pricing && curve_nodes == 0;
                assert_eq!(
                    GreekMethodRequest.is_short(short_pricing, curve_nodes),
                    curve_nodes <= 1024,
                );
                assert!(!solver.is_short(short_pricing, curve_nodes));
                assert!(!NumericalGreeksRequest(greeks).is_short(short_pricing, curve_nodes));
                assert_eq!(
                    ScalarRequest::Price.is_short(short_pricing, curve_nodes),
                    short_pricing
                );
                assert_eq!(greeks.is_short(short_pricing, curve_nodes), short_pricing);
                assert!(!SeededPriceRequest.is_short(short_pricing, curve_nodes));
                assert!(!StatisticsRequest.is_short(short_pricing, curve_nodes));
            }
        }
    }
}
