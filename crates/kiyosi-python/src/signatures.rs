use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

pub(crate) fn register(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    let defaults = PyDict::new(py);
    macro_rules! register {
        ([$($name:ident),+ $(,)?], $($field:ident = $value:expr),+ $(,)?) => {{
            let values = PyDict::new(py);
            $(values.set_item(stringify!($field), $value)?;)+
            $(defaults.set_item(stringify!($name), &values)?;)+
        }};
    }

    register!(
        [GeometricAveragePriceOption, ArithmeticAveragePriceOption],
        realized_average = kiyosi::DEFAULT_REALIZED_AVERAGE,
    );
    register!(
        [BarrierOption, DoubleBarrierOption],
        rebate = kiyosi::BarrierOptionTerms::DEFAULT_REBATE,
        rebate_timing = crate::conversions::rebate_timing_name(kiyosi::RebateTiming::default()),
    );
    register!(
        [TouchOption, DoubleTouchOption],
        settlement_timing =
            crate::conversions::settlement_timing_name(kiyosi::SettlementTiming::default()),
    );
    register!(
        [
            SnowballOption,
            BinarySnowballOption,
            TernarySnowballOption,
            standard_snowball,
            european_snowball,
            step_down_snowball,
            both_down_snowball,
            dual_coupon_snowball,
            parachute_snowball,
            otm_snowball,
            loss_capped_snowball,
            early_profit_snowball,
            butterfly_snowball,
        ],
        principal_redemption_ratio = kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO
    );
    register!(
        [
            PhoenixOption,
            fixed_coupon_note,
            standard_phoenix,
            step_down_phoenix,
            digital_coupon_note,
        ],
        principal_redemption_ratio = kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
        lockout_periods = kiyosi::DEFAULT_PHOENIX_LOCKOUT_PERIODS,
    );
    register!(
        [Accumulator],
        option_type =
            crate::conversions::direction_name(kiyosi::AccumulatorTerms::DEFAULT_OPTION_TYPE),
        knock_out_settled = kiyosi::AccumulatorTerms::DEFAULT_KNOCK_OUT_SETTLED,
    );
    // Schedule-taking callables retain Ellipsis to inherit the supplied calendar.
    register!(
        [
            RangeAccrualOption,
            fixed_interval_schedule,
            monthly_schedule
        ],
        calendar = crate::calendar::TradingCalendar::default(),
    );
    register!(
        [custom_calendar],
        closed_dates = PyTuple::empty(py),
        open_dates = PyTuple::empty(py),
    );

    let binomial = kiyosi::BinomialSettings::default();
    register!(
        [
            CoxRossRubinsteinVanillaEngine,
            CoxRossRubinsteinDigitalEngine,
            CoxRossRubinsteinBarrierEngine,
        ],
        step_count = binomial.step_count
    );
    let quadrature = kiyosi::QuadratureSettings::default();
    register!(
        [
            QuadratureBarrierEngine,
            QuadratureDigitalEngine,
            QuadratureAutocallableEngine,
            QuadratureAccumulatorEngine,
        ],
        asset_step_count = quadrature.asset_step_count,
        integration_standard_deviations = quadrature.integration_standard_deviations,
    );
    let vanilla = kiyosi::QuadratureVanillaSettings::default();
    register!(
        [QuadratureVanillaEngine],
        asset_step_count = vanilla.asset_step_count,
        time_step_count = vanilla.time_step_count,
        integration_standard_deviations = vanilla.integration_standard_deviations,
    );
    let fd = kiyosi::FiniteDifferenceSettings::default();
    register!(
        [
            FiniteDifferenceVanillaEngine,
            FiniteDifferenceDigitalEngine,
            FiniteDifferenceBarrierEngine,
            FiniteDifferenceDoubleBarrierEngine,
            FiniteDifferenceBinaryBarrierEngine,
            FiniteDifferenceDoubleTouchEngine,
            FiniteDifferenceAutocallableEngine,
            FiniteDifferenceAccumulatorEngine,
        ],
        asset_step_count = fd.asset_step_count,
        time_step_count = fd.time_step_count,
        scheme = crate::engines::finite_difference::scheme_name(fd.scheme),
        volatility_shift = fd.greek_shifts.volatility_shift,
        rate_shift = fd.greek_shifts.rate_shift,
    );
    let heston_fd = kiyosi::HestonFiniteDifferenceSettings::default();
    register!(
        [FiniteDifferenceHestonVanillaEngine],
        asset_step_count = heston_fd.asset_step_count,
        variance_step_count = heston_fd.variance_step_count,
        time_step_count = heston_fd.time_step_count,
    );
    let heston = kiyosi::HestonIntegrationSettings::default();
    register!(
        [AnalyticHestonEuropeanEngine],
        absolute_tolerance = heston.absolute_tolerance,
        relative_tolerance = heston.relative_tolerance,
        max_evaluations = heston.max_evaluations,
    );
    let mc = kiyosi::MonteCarloGridSettings::default();
    register!(
        [
            MonteCarloDigitalEngine,
            MonteCarloBarrierEngine,
            MonteCarloAutocallableEngine,
            MonteCarloAccumulatorEngine,
        ],
        path_count = mc.sampling.path_count,
        max_active_workers = mc.sampling.max_active_workers,
        sampling = crate::engines::monte_carlo::sampling_name(mc.sampling.sampling),
        antithetic = mc.sampling.antithetic,
    );
    register!(
        [MonteCarloVanillaEngine, MonteCarloAsianEngine],
        path_count = mc.sampling.path_count,
        step_count = mc.step_count,
        max_active_workers = mc.sampling.max_active_workers,
        sampling = crate::engines::monte_carlo::sampling_name(mc.sampling.sampling),
        antithetic = mc.sampling.antithetic,
    );

    register!(
        [implied_volatility],
        price_tolerance = kiyosi::ImpliedVolatilitySettings::DEFAULT_PRICE_TOLERANCE,
        parameter_tolerance = kiyosi::ImpliedVolatilitySettings::DEFAULT_PARAMETER_TOLERANCE,
        max_iterations = kiyosi::ImpliedVolatilitySettings::DEFAULT_MAX_ITERATIONS,
    );
    let coupon = kiyosi::ImpliedCouponSettings::default();
    register!(
        [implied_snowball_coupon, implied_phoenix_coupon],
        lower_bound = coupon.lower_bound,
        upper_bound = coupon.upper_bound,
        price_tolerance = coupon.price_tolerance,
        parameter_tolerance = coupon.parameter_tolerance,
        max_iterations = coupon.max_iterations,
    );
    module.add("_signature_defaults", defaults)
}
