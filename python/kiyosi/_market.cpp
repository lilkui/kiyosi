#include "_binding.hpp"

#include <chrono>

using namespace nb::literals;

namespace kiyosi::python_binding {

void bind_enums(nb::module_& module)
{
    nb::enum_<BusinessDayConvention>(module, "BusinessDayConvention",
                                     "Rule for adjusting a nominal date to a trading day.")
        .value("FOLLOWING", BusinessDayConvention::following)
        .value("PRECEDING", BusinessDayConvention::preceding);
    nb::enum_<ErrorCategory>(module, "ErrorCategory", R"doc(Stable category for a core domain error.

Attributes
----------
INVALID_OPTION
    Invalid or internally inconsistent instrument terms.
INVALID_STRIKE
    Invalid strike value.
INVALID_VOLATILITY
    Invalid volatility value.
INVALID_RISK_FREE_RATE
    Invalid risk-free rate.
INVALID_DIVIDEND_YIELD
    Invalid dividend yield.
INVALID_SPOT_PRICE
    Invalid spot price.
INVALID_DATE
    Invalid calendar date.
INVALID_TIME_RANGE
    Invalid or reversed time range.
INVALID_RESULT
    Missing or invalid pricing result.
INVALID_SCHEDULE
    Invalid observation schedule.
INVALID_CALENDAR
    Invalid trading calendar.
INVALID_PARAMETER
    Invalid numerical or engine parameter.
UNBRACKETED_VOLATILITY
    Observed price is not bracketed by the volatility bounds.
SOLVER_NON_CONVERGENCE
    A numerical solver exhausted its iteration limit.
SOLVER_NON_FINITE
    A numerical solver encountered a non-finite value.
UNBRACKETED_COUPON
    Observed price is not bracketed by the coupon bounds.
BACKEND_UNAVAILABLE
    Requested computation backend is unavailable.
BACKEND_FAILURE
    Requested computation backend failed.
UNSUPPORTED_OPERATION
    Engine or instrument does not support the requested operation.)doc")
        .value("INVALID_OPTION", ErrorCategory::invalid_option)
        .value("INVALID_STRIKE", ErrorCategory::invalid_strike)
        .value("INVALID_VOLATILITY", ErrorCategory::invalid_volatility)
        .value("INVALID_RISK_FREE_RATE", ErrorCategory::invalid_risk_free_rate)
        .value("INVALID_DIVIDEND_YIELD", ErrorCategory::invalid_dividend_yield)
        .value("INVALID_SPOT_PRICE", ErrorCategory::invalid_spot_price)
        .value("INVALID_DATE", ErrorCategory::invalid_date)
        .value("INVALID_TIME_RANGE", ErrorCategory::invalid_time_range)
        .value("INVALID_RESULT", ErrorCategory::invalid_result)
        .value("INVALID_SCHEDULE", ErrorCategory::invalid_schedule)
        .value("INVALID_CALENDAR", ErrorCategory::invalid_calendar)
        .value("INVALID_PARAMETER", ErrorCategory::invalid_parameter)
        .value("UNBRACKETED_VOLATILITY", ErrorCategory::unbracketed_volatility)
        .value("SOLVER_NON_CONVERGENCE", ErrorCategory::solver_non_convergence)
        .value("SOLVER_NON_FINITE", ErrorCategory::solver_non_finite)
        .value("UNBRACKETED_COUPON", ErrorCategory::unbracketed_coupon)
        .value("BACKEND_UNAVAILABLE", ErrorCategory::backend_unavailable)
        .value("BACKEND_FAILURE", ErrorCategory::backend_failure)
        .value("UNSUPPORTED_OPERATION", ErrorCategory::unsupported_operation);
    nb::enum_<OptionType>(module, "OptionType", R"doc(Option payoff direction.

Attributes
----------
CALL
    Right to benefit from prices above the strike.
PUT
    Right to benefit from prices below the strike.)doc")
        .value("CALL", OptionType::call)
        .value("PUT", OptionType::put);
    nb::enum_<BarrierType>(module, "BarrierType", R"doc(Direction and activation behavior of a barrier.

Attributes
----------
UP_AND_IN
    Activate when spot reaches an upper barrier.
UP_AND_OUT
    Terminate when spot reaches an upper barrier.
DOWN_AND_IN
    Activate when spot reaches a lower barrier.
DOWN_AND_OUT
    Terminate when spot reaches a lower barrier.)doc")
        .value("UP_AND_IN", BarrierType::up_and_in)
        .value("UP_AND_OUT", BarrierType::up_and_out)
        .value("DOWN_AND_IN", BarrierType::down_and_in)
        .value("DOWN_AND_OUT", BarrierType::down_and_out);
    nb::enum_<ObservationMode>(module, "ObservationMode", R"doc(Barrier observation frequency.

Attributes
----------
CONTINUOUS
    Observe throughout the instrument lifetime.
SCHEDULED
    Observe only on the supplied observation dates.)doc")
        .value("CONTINUOUS", ObservationMode::continuous)
        .value("SCHEDULED", ObservationMode::scheduled);
    nb::enum_<RebateTiming>(module, "RebateTiming", R"doc(Payment time for a barrier-option rebate.

Attributes
----------
AT_HIT
    Pay when the barrier is hit.
AT_EXPIRY
    Pay at expiry.)doc")
        .value("AT_HIT", RebateTiming::at_hit)
        .value("AT_EXPIRY", RebateTiming::at_expiry);
    nb::enum_<SettlementTiming>(module, "SettlementTiming", R"doc(Settlement time for a one-touch payoff.

Attributes
----------
AT_HIT
    Settle when the barrier is hit.
AT_EXPIRY
    Settle at expiry.)doc")
        .value("AT_HIT", SettlementTiming::at_hit)
        .value("AT_EXPIRY", SettlementTiming::at_expiry);
    nb::enum_<PayoffType>(module, "PayoffType", R"doc(Delivery form of a binary payoff.

Attributes
----------
CASH
    Deliver a fixed cash amount.
ASSET
    Deliver the underlying asset value.)doc")
        .value("CASH", PayoffType::cash)
        .value("ASSET", PayoffType::asset);
    nb::enum_<KnockInObservationMode>(module, "KnockInObservationMode", R"doc(Observation rule for an autocallable knock-in barrier.

Attributes
----------
EVERY_TRADING_DAY
    Observe on every trading day through expiry.
AT_EXPIRY
    Observe only at expiry.)doc")
        .value("EVERY_TRADING_DAY", KnockInObservationMode::every_trading_day)
        .value("AT_EXPIRY", KnockInObservationMode::at_expiry);
    nb::enum_<AutocallableBarrierState>(module, "AutocallableBarrierState", R"doc(Barrier history known at valuation time.

Attributes
----------
NONE
    No barrier event has occurred.
KNOCKED_OUT
    The instrument has already knocked out.
KNOCKED_IN
    The knock-in barrier has already been breached.)doc")
        .value("NONE", AutocallableBarrierState::none)
        .value("KNOCKED_OUT", AutocallableBarrierState::knocked_out)
        .value("KNOCKED_IN", AutocallableBarrierState::knocked_in);
    nb::enum_<FiniteDifferenceScheme>(module, "FiniteDifferenceScheme", R"doc(Time-stepping scheme for finite-difference engines.

Attributes
----------
EXPLICIT_EULER
    Explicit Euler time stepping.
IMPLICIT_EULER
    Implicit Euler time stepping.
CRANK_NICOLSON
    Crank-Nicolson time stepping.)doc")
        .value("EXPLICIT_EULER", FiniteDifferenceScheme::explicit_euler)
        .value("IMPLICIT_EULER", FiniteDifferenceScheme::implicit_euler)
        .value("CRANK_NICOLSON", FiniteDifferenceScheme::crank_nicolson);
    nb::enum_<MonteCarloBackend>(module, "MonteCarloBackend", R"doc(Execution backend for Monte Carlo engines.

Attributes
----------
CPU
    Run on the host processor.
CUDA
    Run on a CUDA-capable GPU.)doc")
        .value("CPU", MonteCarloBackend::cpu)
        .value("CUDA", MonteCarloBackend::cuda);
    nb::enum_<CouponQuoteConvention>(module, "CouponQuoteConvention", R"doc(Coupon component varied by a snowball implied-coupon solve.

Attributes
----------
SHIFT_MATURITY_COUPON
    Shift the maturity coupon with the quoted knock-out coupons.
PRESERVE_MATURITY_COUPON
    Keep the maturity coupon fixed while shifting knock-out coupons.)doc")
        .value("SHIFT_MATURITY_COUPON", CouponQuoteConvention::shift_maturity_coupon)
        .value("PRESERVE_MATURITY_COUPON", CouponQuoteConvention::preserve_maturity_coupon);
    nb::enum_<RiskMeasure>(module, "RiskMeasure", R"doc(Risk measure stored by :class:`PricingResult`.

Attributes
----------
PRICE, DELTA, GAMMA, SPEED, THETA, CHARM, COLOR, VEGA, VANNA, ZOMMA, RHO
    Price or a supported first-, second-, or third-order sensitivity.)doc")
        .value("PRICE", RiskMeasure::price)
        .value("DELTA", RiskMeasure::delta)
        .value("GAMMA", RiskMeasure::gamma)
        .value("SPEED", RiskMeasure::speed)
        .value("THETA", RiskMeasure::theta)
        .value("CHARM", RiskMeasure::charm)
        .value("COLOR", RiskMeasure::color)
        .value("VEGA", RiskMeasure::vega)
        .value("VANNA", RiskMeasure::vanna)
        .value("ZOMMA", RiskMeasure::zomma)
        .value("RHO", RiskMeasure::rho);
}

void bind_market(nb::module_& module)
{
    auto parameters = nb::class_<BlackScholesMertonParameters>(
        module, "BlackScholesMertonParameters", R"doc(Validated Black-Scholes-Merton market parameters.

Parameters are continuously compounded decimal rates and an annualized decimal
volatility. Instances are immutable value objects.

Attributes
----------
risk_free_rate : float
    Continuously compounded annual risk-free rate.
dividend_yield : float
    Continuously compounded annual dividend yield.
volatility : float
    Positive annualized volatility.)doc")
        .def(nb::new_([](PythonReal risk_free_rate, PythonReal dividend_yield,
                        PythonReal volatility) {
                 return unwrap(make_bsm_parameters(
                     real_number(risk_free_rate, "risk_free_rate"),
                     real_number(dividend_yield, "dividend_yield"),
                     real_number(volatility, "volatility")));
             }),
             nb::kw_only(), "risk_free_rate"_a, "dividend_yield"_a, "volatility"_a,
             R"doc(Create validated Black-Scholes-Merton parameters.

Parameters
----------
risk_free_rate : float
    Continuously compounded annual risk-free rate.
dividend_yield : float
    Continuously compounded annual dividend yield.
volatility : float
    Positive annualized volatility.

Raises
------
TypeError
    If a value is not a real number.
OverflowError
    If a value cannot be represented as a C++ ``double``.
KiyosiError
    If a value is non-finite or volatility is not positive.)doc")
        .def_prop_ro("risk_free_rate", &BlackScholesMertonParameters::risk_free_rate,
                     "Continuously compounded annual risk-free rate.")
        .def_prop_ro("dividend_yield", &BlackScholesMertonParameters::dividend_yield,
                     "Continuously compounded annual dividend yield.")
        .def_prop_ro("volatility", &BlackScholesMertonParameters::volatility,
                     "Positive annualized volatility.");
    bind_value_equality(parameters);
    bind_repr(parameters, "BlackScholesMertonParameters",
              {{"risk_free_rate", "risk_free_rate"},
               {"dividend_yield", "dividend_yield"},
               {"volatility", "volatility"}});

    auto calendar = nb::class_<TradingCalendar>(
        module, "TradingCalendar",
        R"doc(Read-only trading-day calendar.

Instances are created by :func:`all_days_calendar`, :func:`weekdays_calendar`,
or :func:`sse_calendar`.

Attributes
----------
trading_days_per_year : int
    Annualization denominator used for trading-year fractions.)doc")
        .def("is_trading_day",
             [](const TradingCalendar& calendar, PythonDate value) {
                 return calendar.is_trading_day(calendar_date(value, "value"));
             },
             "value"_a, R"doc(Return whether a date is a trading day.

Parameters
----------
value : datetime.date
    Calendar date to inspect.

Returns
-------
bool
    ``True`` when the date is open for trading.

Raises
------
TypeError
    If ``value`` is not a :class:`datetime.date`.)doc")
        .def("adjust",
             [](const TradingCalendar& calendar, PythonDate nominal,
                BusinessDayConvention convention) {
                 return python_date(unwrap(calendar.adjust(calendar_date(nominal, "nominal"), convention)));
             },
             "nominal"_a, "convention"_a,
             "Adjust a nominal date to a trading day using FOLLOWING or PRECEDING.")
        .def("trading_days_between",
             [](const TradingCalendar& calendar, PythonDate start, PythonDate end) {
                 return unwrap(calendar.trading_days_between(
                     calendar_date(start, "start"), calendar_date(end, "end")));
             },
             "start"_a, "end"_a, R"doc(Count trading days in the half-open interval ``[start, end)``.

Parameters
----------
start : datetime.date
    First date included in the interval.
end : datetime.date
    Exclusive interval end.

Returns
-------
int
    Number of trading days.

Raises
------
TypeError
    If either argument is not a :class:`datetime.date`.
KiyosiError
    If the range is reversed.)doc")
        .def("trading_year_fraction",
             [](const TradingCalendar& calendar, PythonDate start, PythonDate end) {
                 return unwrap(calendar.trading_year_fraction(
                     calendar_date(start, "start"), calendar_date(end, "end")));
             },
             "start"_a, "end"_a, R"doc(Return the trading-year fraction for ``[start, end)``.

Parameters
----------
start : datetime.date
    First date included in the interval.
end : datetime.date
    Exclusive interval end.

Returns
-------
float
    Trading-day count divided by :attr:`trading_days_per_year`.

Raises
------
TypeError
    If either argument is not a :class:`datetime.date`.
KiyosiError
    If the range is reversed.)doc")
        .def_prop_ro("trading_days_per_year", &TradingCalendar::trading_days_per_year,
                     "Annualization denominator for trading-year fractions.");
    bind_repr(calendar, "TradingCalendar",
              {{"trading_days_per_year", "trading_days_per_year"}});

    auto schedule = nb::class_<ObservationSchedule>(
        module, "ObservationSchedule", R"doc(Immutable ordered observation dates.

The sequence supports ``len(schedule)``, integer indexing, negative indexing,
and iteration.

Attributes
----------
dates : list[datetime.date]
    Copy of the ordered observation dates.)doc")
        .def("__len__", &ObservationSchedule::size)
        .def("__getitem__", [](const ObservationSchedule& schedule, nb::ssize_t index) {
            const auto size = static_cast<nb::ssize_t>(schedule.size());
            if (index < 0) index += size;
            if (index < 0 || index >= size) throw nb::index_error();
            return python_date(schedule[static_cast<std::size_t>(index)]);
        })
        .def("__iter__", [](const ObservationSchedule& schedule) {
            PythonDateList output;
            for (const Date value : schedule.dates()) output.append(python_date(value));
            return PythonDateIterator{output.attr("__iter__")()};
        })
        .def_prop_ro("dates", [](const ObservationSchedule& schedule) {
            PythonDateList output;
            for (const Date value : schedule.dates()) output.append(python_date(value));
            return output;
        }, "Copy of the ordered observation dates.");
    bind_value_equality(schedule);
    bind_repr(schedule, "ObservationSchedule", {{"dates", "dates"}});

    auto context = nb::class_<PricingContext>(
        module, "PricingContext", R"doc(Validated market state for a valuation instant.

Attributes
----------
model_parameters : BlackScholesMertonParameters
    Read-only parameters view tied to this context's lifetime.
spot_price : float
    Positive underlying spot price.
valuation_date : datetime.date
    UTC calendar date containing the valuation instant.
valuation_time : datetime.datetime
    Timezone-aware valuation timestamp normalized to UTC.
calendar : TradingCalendar
    Read-only calendar view tied to this context's lifetime.)doc")
        .def(nb::new_([](const BlackScholesMertonParameters& parameters, PythonReal spot_price,
                        PythonValuationTime time, const TradingCalendar& calendar) {
                 return unwrap(make_pricing_context(
                     parameters, real_number(spot_price, "spot_price"),
                     valuation_time(time), calendar));
             }),
             nb::kw_only(), "model_parameters"_a, "spot_price"_a, "valuation_time"_a,
             "calendar"_a = weekdays_calendar(),
             R"doc(Create a validated pricing context.

Parameters
----------
model_parameters : BlackScholesMertonParameters
    Black-Scholes-Merton market parameters.
spot_price : float
    Positive underlying spot price.
valuation_time : datetime.date or datetime.datetime
    Valuation instant. Dates denote midnight UTC; datetimes must be timezone
    aware and are normalized to UTC.
calendar : TradingCalendar, optional
    Trading calendar. Defaults to :func:`weekdays_calendar`.

Raises
------
TypeError
    If an argument has an incompatible representation or a datetime is naive.
KiyosiError
    If the core rejects the spot price or valuation state.)doc")
        .def_prop_ro("model_parameters", &PricingContext::model_parameters,
                     nb::rv_policy::reference_internal,
                     "Read-only parameters view that keeps this context alive; concurrent reads are safe.")
        .def_prop_ro("spot_price", &PricingContext::spot_price,
                     "Positive underlying spot price.")
        .def_prop_ro("valuation_date", [](const PricingContext& context) {
            return python_date(context.valuation_date());
        }, "UTC date containing the valuation instant.")
        .def_prop_ro("valuation_time", [](const PricingContext& context) {
            return python_timestamp(context.valuation_time());
        }, "Timezone-aware valuation timestamp normalized to UTC.")
        .def_prop_ro("calendar", &PricingContext::calendar,
                     nb::rv_policy::reference_internal,
                     "Read-only calendar view that keeps this context alive; concurrent reads are safe.");
    bind_repr(context, "PricingContext",
              {{"model_parameters", "model_parameters"}, {"spot_price", "spot_price"},
               {"valuation_time", "valuation_time"}, {"calendar", "calendar"}});

    module.def("all_days_calendar", &all_days_calendar, R"doc(Return a calendar in which every day is a trading day.

Returns
-------
TradingCalendar
    Calendar with 365 annual trading days.)doc");
    module.def("weekdays_calendar", &weekdays_calendar, R"doc(Return a holiday-unaware weekdays calendar.

Returns
-------
TradingCalendar
    Monday-to-Friday calendar with 252 annual trading days.)doc");
    module.def("sse_calendar", &sse_calendar, R"doc(Return the Shanghai Stock Exchange holiday calendar.

Returns
-------
TradingCalendar
    Exchange calendar with 252 annual trading days.)doc");
    module.def(
        "fixed_interval_schedule",
        [](PythonDate start, PythonDate end, PythonInteger interval_days,
           const TradingCalendar& calendar) {
            return unwrap(make_fixed_interval_schedule(
                calendar_date(start, "start"), calendar_date(end, "end"),
                std::chrono::days{integer(interval_days, "interval_days")}, calendar));
        },
        nb::kw_only(), "start"_a, "end"_a, "interval_days"_a,
        "calendar"_a = weekdays_calendar(),
        R"doc(Build a fixed-calendar-day observation schedule.

Candidates are ``start + n * interval_days`` for positive ``n``. start is excluded
and ``end`` is an inclusive bound. Candidates use following
trading-day adjustment, duplicate adjusted dates are removed, and generation
stops rather than crossing ``end``; ``end`` is not guaranteed.

Parameters
----------
start : datetime.date
    Anchor date, excluded from the result.
end : datetime.date
    Inclusive upper bound for adjusted dates.
interval_days : int
    Positive number of calendar days between candidates.
calendar : TradingCalendar, optional
    Adjustment calendar. Defaults to :func:`weekdays_calendar`.

Returns
-------
ObservationSchedule
    Immutable adjusted observation dates.

Raises
------
TypeError
    If an argument has an incompatible representation.
OverflowError
    If ``interval_days`` is outside the C++ ``int`` range.
KiyosiError
    If the dates, interval, calendar, or resulting schedule are invalid.

Examples
--------
With the weekdays calendar, 2025-01-03 through 2025-01-07 at a one-day
interval produces 2025-01-06 and 2025-01-07. Supply explicit
``observation_dates`` to an instrument constructor for bespoke terminal dates.)doc");
    module.def(
        "monthly_schedule",
        [](PythonDate start, PythonDate end, PythonInteger lock_up_months,
           const TradingCalendar& calendar) {
            return unwrap(make_monthly_schedule(
                calendar_date(start, "start"), calendar_date(end, "end"),
                integer(lock_up_months, "lock_up_months"), calendar));
        },
        nb::kw_only(), "start"_a, "end"_a, "lock_up_months"_a,
        "calendar"_a = weekdays_calendar(),
        R"doc(Build a monthly observation schedule after a lock-up period.

Monthly candidates begin at ``start + lock_up_months``. ``start`` is excluded
and ``end`` is an inclusive bound. The start day is clamped to each target
month's last day, then candidates use following trading-day adjustment.
Generation stops rather than crossing ``end``, so ``end`` is not guaranteed.

Parameters
----------
start : datetime.date
    Anchor date, excluded from the result.
end : datetime.date
    Inclusive upper bound for adjusted dates.
lock_up_months : int
    Positive number of months before the first candidate.
calendar : TradingCalendar, optional
    Adjustment calendar. Defaults to :func:`weekdays_calendar`.

Returns
-------
ObservationSchedule
    Immutable adjusted observation dates.

Raises
------
TypeError
    If an argument has an incompatible representation.
OverflowError
    If ``lock_up_months`` is outside the C++ ``int`` range.
KiyosiError
    If the dates, lock-up, calendar, or resulting schedule are invalid.

Examples
--------
With the weekdays calendar, 2025-01-01 through 2025-03-01 with one lock-up
month produces only 2025-02-03; adjusting the Saturday end candidate would
cross the bound. Supply explicit ``observation_dates`` to an instrument
constructor for bespoke terminal dates.)doc");
}

} // namespace kiyosi::python_binding
