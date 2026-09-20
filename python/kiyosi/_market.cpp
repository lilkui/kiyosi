#include "_binding.hpp"

#include <chrono>

using namespace nb::literals;

namespace kiyosi::python_binding {

void bind_enums(nb::module_& module)
{
    nb::enum_<ErrorCategory>(module, "ErrorCategory")
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
    nb::enum_<OptionType>(module, "OptionType")
        .value("CALL", OptionType::call)
        .value("PUT", OptionType::put);
    nb::enum_<BarrierType>(module, "BarrierType")
        .value("UP_AND_IN", BarrierType::up_and_in)
        .value("UP_AND_OUT", BarrierType::up_and_out)
        .value("DOWN_AND_IN", BarrierType::down_and_in)
        .value("DOWN_AND_OUT", BarrierType::down_and_out);
    nb::enum_<ObservationMode>(module, "ObservationMode")
        .value("CONTINUOUS", ObservationMode::continuous)
        .value("SCHEDULED", ObservationMode::scheduled);
    nb::enum_<RebateTiming>(module, "RebateTiming")
        .value("AT_HIT", RebateTiming::at_hit)
        .value("AT_EXPIRY", RebateTiming::at_expiry);
    nb::enum_<SettlementTiming>(module, "SettlementTiming")
        .value("AT_HIT", SettlementTiming::at_hit)
        .value("AT_EXPIRY", SettlementTiming::at_expiry);
    nb::enum_<PayoffType>(module, "PayoffType")
        .value("CASH", PayoffType::cash)
        .value("ASSET", PayoffType::asset);
    nb::enum_<KnockInObservationMode>(module, "KnockInObservationMode")
        .value("EVERY_TRADING_DAY", KnockInObservationMode::every_trading_day)
        .value("AT_EXPIRY", KnockInObservationMode::at_expiry);
    nb::enum_<AutocallableBarrierState>(module, "AutocallableBarrierState")
        .value("NONE", AutocallableBarrierState::none)
        .value("KNOCKED_OUT", AutocallableBarrierState::knocked_out)
        .value("KNOCKED_IN", AutocallableBarrierState::knocked_in);
    nb::enum_<FiniteDifferenceScheme>(module, "FiniteDifferenceScheme")
        .value("EXPLICIT_EULER", FiniteDifferenceScheme::explicit_euler)
        .value("IMPLICIT_EULER", FiniteDifferenceScheme::implicit_euler)
        .value("CRANK_NICOLSON", FiniteDifferenceScheme::crank_nicolson);
    nb::enum_<MonteCarloBackend>(module, "MonteCarloBackend")
        .value("CPU", MonteCarloBackend::cpu)
        .value("CUDA", MonteCarloBackend::cuda);
    nb::enum_<CouponQuoteConvention>(module, "CouponQuoteConvention")
        .value("SHIFT_MATURITY_COUPON", CouponQuoteConvention::shift_maturity_coupon)
        .value("PRESERVE_MATURITY_COUPON", CouponQuoteConvention::preserve_maturity_coupon);
    nb::enum_<RiskMeasure>(module, "RiskMeasure")
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
        module, "BlackScholesMertonParameters", "Validated Black-Scholes-Merton market parameters.")
        .def(nb::new_([](PythonReal risk_free_rate, PythonReal dividend_yield,
                        PythonReal volatility) {
                 return unwrap(make_bsm_parameters(
                     real_number(risk_free_rate, "risk_free_rate"),
                     real_number(dividend_yield, "dividend_yield"),
                     real_number(volatility, "volatility")));
             }),
             nb::kw_only(), "risk_free_rate"_a, "dividend_yield"_a, "volatility"_a,
             "Create validated continuously compounded rates and volatility.")
        .def_prop_ro("risk_free_rate", &BlackScholesMertonParameters::risk_free_rate)
        .def_prop_ro("dividend_yield", &BlackScholesMertonParameters::dividend_yield)
        .def_prop_ro("volatility", &BlackScholesMertonParameters::volatility);
    bind_value_equality(parameters);
    bind_repr(parameters, "BlackScholesMertonParameters",
              {{"risk_free_rate", "risk_free_rate"},
               {"dividend_yield", "dividend_yield"},
               {"volatility", "volatility"}});

    auto calendar = nb::class_<TradingCalendar>(
        module, "TradingCalendar",
        "Read-only trading-day calendar. Instances are created by calendar factories.")
        .def("is_trading_day",
             [](const TradingCalendar& calendar, PythonDate value) {
                 return calendar.is_trading_day(calendar_date(value, "value"));
             },
             "value"_a, "Return whether value is a trading day.")
        .def("trading_days_between",
             [](const TradingCalendar& calendar, PythonDate start, PythonDate end) {
                 return unwrap(calendar.trading_days_between(
                     calendar_date(start, "start"), calendar_date(end, "end")));
             },
             "start"_a, "end"_a,
             "Count trading days in [start, end); reversed ranges are rejected.")
        .def("trading_year_fraction",
             [](const TradingCalendar& calendar, PythonDate start, PythonDate end) {
                 return unwrap(calendar.trading_year_fraction(
                     calendar_date(start, "start"), calendar_date(end, "end")));
             },
             "start"_a, "end"_a,
             "Return trading days in [start, end) divided by trading_days_per_year.")
        .def_prop_ro("trading_days_per_year", &TradingCalendar::trading_days_per_year);
    bind_repr(calendar, "TradingCalendar",
              {{"trading_days_per_year", "trading_days_per_year"}});

    auto schedule = nb::class_<ObservationSchedule>(
        module, "ObservationSchedule", "Immutable ordered observation dates.")
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
        });
    bind_value_equality(schedule);
    bind_repr(schedule, "ObservationSchedule", {{"dates", "dates"}});

    auto context = nb::class_<PricingContext>(
        module, "PricingContext", "Validated market state for a valuation instant.")
        .def(nb::new_([](const BlackScholesMertonParameters& parameters, PythonReal spot_price,
                        PythonValuationTime time, const TradingCalendar& calendar) {
                 return unwrap(make_pricing_context(
                     parameters, real_number(spot_price, "spot_price"),
                     valuation_time(time), calendar));
             }),
             nb::kw_only(), "model_parameters"_a, "spot_price"_a, "valuation_time"_a,
             "calendar"_a = weekdays_calendar(),
             "Create a pricing context; dates denote midnight UTC and use the weekdays calendar by default.")
        .def_prop_ro("model_parameters", &PricingContext::model_parameters,
                     nb::rv_policy::reference_internal,
                     "Read-only parameters view that keeps this context alive; concurrent reads are safe.")
        .def_prop_ro("spot_price", &PricingContext::spot_price)
        .def_prop_ro("valuation_date", [](const PricingContext& context) {
            return python_date(context.valuation_date());
        })
        .def_prop_ro("valuation_time", [](const PricingContext& context) {
            return python_timestamp(context.valuation_time());
        })
        .def_prop_ro("calendar", &PricingContext::calendar,
                     nb::rv_policy::reference_internal,
                     "Read-only calendar view that keeps this context alive; concurrent reads are safe.");
    bind_repr(context, "PricingContext",
              {{"model_parameters", "model_parameters"}, {"spot_price", "spot_price"},
               {"valuation_time", "valuation_time"}, {"calendar", "calendar"}});

    module.def("all_days_calendar", &all_days_calendar,
               "Return a 365-day calendar in which every day is a trading day.");
    module.def("weekdays_calendar", &weekdays_calendar,
               "Return a holiday-unaware Monday-to-Friday calendar with 252 annual trading days.");
    module.def("sse_calendar", &sse_calendar,
               "Return the Shanghai Stock Exchange holiday calendar with 252 annual trading days.");
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
        "Build candidates at start + n * interval_days for positive n. start is excluded and "
        "end is an inclusive bound. Candidates use following trading-day adjustment, duplicate "
        "adjusted dates are removed, and generation stops rather than crossing end; end is not "
        "guaranteed. With the weekdays calendar, 2025-01-03 through 2025-01-07 at a one-day "
        "interval produces [2025-01-06, 2025-01-07]. Supply explicit observation_dates to an "
        "instrument constructor for bespoke terminal dates.");
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
        "Build monthly candidates beginning at start + lock_up_months. start is excluded and "
        "end is an inclusive bound. The start day is clamped to each target month's last day, "
        "then candidates use following trading-day adjustment. Generation stops rather than "
        "crossing end, so end is not guaranteed. With the weekdays calendar, 2025-01-01 through "
        "2025-03-01 with one lock-up month produces [2025-02-03]; the Saturday end candidate "
        "would cross the bound. Supply explicit observation_dates to an instrument constructor "
        "for bespoke terminal dates.");
}

} // namespace kiyosi::python_binding
