macro_rules! engine_methods {
    ($name:ident, $($method:item),* $(,)?) => {
        #[pyo3::pymethods]
        impl $name {
            $($method)*
        }
    };
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
            $(#[$extra_doc])*
            #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false, spot_shift=kiyosi::NumericalShiftSettings::default().spot_shift, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
            #[expect(clippy::too_many_arguments, reason = "explicit Python pricing controls")]
            fn price_with_greeks(
                &self,
                py: pyo3::Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &$crate::market::PricingContext,
                greeks: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
                #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
                #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
                #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
            ) -> pyo3::PyResult<$crate::risk::PricingResult> {
                let $request = $crate::risk::greeks_request(greeks, all_greeks)?;
                let $shifts = kiyosi::NumericalShiftSettings {
                    spot_shift, volatility_shift, rate_shift, time_shift_days,
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

impl ScalarRequest {
    pub(crate) fn evaluate<I: kiyosi::OptionContract>(
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
