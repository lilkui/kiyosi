use kiyosi::binding_support::{
    DEFAULT_AUTOCALLABLE_REBATE_RATE, DEFAULT_PHOENIX_LOCKOUT_PERIODS,
    DEFAULT_PRINCIPAL_REDEMPTION_RATIO, DEFAULT_REALIZED_AVERAGE,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

pub(crate) fn register(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    let defaults = PyDict::new(py);
    macro_rules! register {
        ([$($name:ident $(.$method:ident)?),+ $(,)?], $($field:ident = $value:expr),+ $(,)?) => {{
            let values = PyDict::new(py);
            $(values.set_item(stringify!($field), $value)?;)+
            $(defaults.set_item(concat!(stringify!($name) $(, ".", stringify!($method))?), &values)?;)+
        }};
    }

    register!(
        [BlackScholesMertonParameters, LocalVolatilityParameters],
        variance_time_convention = crate::market::variance_time_convention_name(
            &kiyosi::VarianceTimeConvention::default()
        ),
    );
    register!(
        [GeometricAveragePriceOption, ArithmeticAveragePriceOption],
        realized_average = DEFAULT_REALIZED_AVERAGE,
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
            SnowballOption.standard,
            SnowballOption.european,
            SnowballOption.step_down,
            SnowballOption.both_down,
            SnowballOption.dual_coupon,
            SnowballOption.parachute,
            SnowballOption.otm,
            SnowballOption.loss_capped,
            SnowballOption.early_profit,
            SnowballOption.butterfly,
        ],
        rebate_rate = DEFAULT_AUTOCALLABLE_REBATE_RATE,
        annual_rebate_rate = DEFAULT_AUTOCALLABLE_REBATE_RATE,
        principal_redemption_ratio = DEFAULT_PRINCIPAL_REDEMPTION_RATIO
    );
    register!(
        [
            PhoenixOption.fixed_coupon,
            PhoenixOption.standard,
            PhoenixOption.step_down,
            PhoenixOption.digital_coupon,
        ],
        rebate_rate = DEFAULT_AUTOCALLABLE_REBATE_RATE,
        annual_rebate_rate = DEFAULT_AUTOCALLABLE_REBATE_RATE,
        principal_redemption_ratio = DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
        lockout_periods = DEFAULT_PHOENIX_LOCKOUT_PERIODS,
    );
    register!(
        [PhoenixOption],
        rebate_rate = DEFAULT_AUTOCALLABLE_REBATE_RATE,
        annual_rebate_rate = DEFAULT_AUTOCALLABLE_REBATE_RATE,
        principal_redemption_ratio = DEFAULT_PRINCIPAL_REDEMPTION_RATIO
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
            ObservationSchedule.fixed_interval,
            ObservationSchedule.monthly,
        ],
        calendar = crate::calendar::TradingCalendar::default(),
    );
    register!(
        [TradingCalendar.custom],
        closed_dates = PyTuple::empty(py),
        open_dates = PyTuple::empty(py),
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
